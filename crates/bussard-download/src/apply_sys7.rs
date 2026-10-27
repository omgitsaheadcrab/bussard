//! Writing the model's link tables to a **System 7** (mask 0705 / 0701) device —
//! the write side of `bussard apply` for the family [`crate::apply`] does not
//! serve.
//!
//! System B opens two loadable interface objects and writes them as property
//! arrays. System 7 has no such objects: the tables are absolute memory regions
//! driven by two of the three parallel load-state machines
//! (`[system7-spec §2/§3]`):
//!
//! - **LSM 1** owns the `0x4000` region — the address table (GrAT);
//! - **LSM 2** owns the `0x4201` region — the association table (GrOAT).
//!
//! So an `apply` here is the same shape as a download, restricted to the two
//! table LSMs: `Unload` both, `StartLoading` both, allocate each region as an
//! absolute Data segment, stream the images in 12-octet `A_Memory_Write` chunks
//! with a read-back verify per chunk, commit a `TaskSegment` descriptor, and
//! `LoadCompleted`. The parameter LSM (3) and the application image are never
//! touched — that is the whole point of the incremental path: a link change must
//! not reset parameters (issue #91).
//!
//! # Op-sequence ordering (decision + rationale)
//!
//! The same coupling as System B: the association table's TSAP column indexes the
//! address table, so a window where one is active against the other's old content
//! would orphan TSAPs. Both LSMs are therefore opened before either region is
//! written, and the address LSM is completed first:
//!
//! 1. `Unload` LSM 2, `Unload` LSM 1 — tear both down (spec §3, every corpus
//!    download unloads before loading).
//! 2. `StartLoading` LSM 2, `StartLoading` LSM 1 — both inactive and writable.
//! 3. Allocate + stream the **0x4000** region (the new address table).
//! 4. Allocate + stream the **0x4201** region (the association table, whose TSAPs
//!    now index the just-written address content).
//! 5. `TaskSegment` + `LoadCompleted` on LSM 1 (activate the address table).
//! 6. `TaskSegment` + `LoadCompleted` on LSM 2 (activate the associations).
//! 7. Switch the communication flags of the objects whose links changed (see
//!    below), each octet read back.
//!
//! A device evaluates group telegrams against a table only in the `Loaded` state
//! (KNX 3/5/1 load-state machine), so no partially-updated table is ever active.
//!
//! # Only the address table is written into the 0x4000 region
//!
//! The group-object descriptor table is not behind the address table: a System 7
//! program declares it in an application segment of its own (the Ekinex EK-E72
//! at `0x41D8`, the Jung `A-2088` at `0x43FF`), and the octets behind the
//! address table are the unused rest of its segment. So `apply` writes the
//! address table and nothing past it, and refuses a table longer than the
//! segment ([`Sys7LiveTables::address_capacity`]), so a longer table can never
//! run into the association or group-object table.
//!
//! # Communication flags
//!
//! A device ignores every telegram for an object whose descriptor has the
//! communication flag (C, bit 2 of CONFIG) cleared, and ETS clears it on every
//! object without a link (the EK-E72 1.1.39: C on exactly its five linked
//! objects; the flash post-pass does the same). A new link on such an object
//! is written and verified but never acts: the EK-E72 kept ignoring a write to
//! its newly linked object 32 until its C flag was set. So, where the product
//! declares the group-object table ([`Sys7LiveTables::group_object_base`]),
//! `apply` sets C on every object the model links that lacks it, and clears it
//! on an object that loses its last link, writing the one CONFIG octet each
//! and reading it back. Nothing else in a descriptor changes. Without product
//! data the table's place is unknown: the plan names the newly linked objects
//! whose flag it could not check ([`Sys7TableImages::unchecked_flags`]).
//!
//! # Restart: not issued
//!
//! Every full System 7 download ends with a restart (`[corpus: 47/49]`,
//! `[system7-spec §6]`), but a restart is **not** part of table activation. The
//! KNX load-state machine activates a table object's content on `LoadCompleted`
//! (§4.1: `Loading --LoadCompleted--> Loaded`), and the corpus restart is the
//! terminal step of a procedure that also rewrote the **application and parameter
//! image** — the thing that genuinely needs a re-init. A table-only write leaves
//! the application untouched, so bussard does not restart: the device keeps
//! running, which is the entire value of the incremental path over a full flash.
//! `S7-CAL: confirm a real 0705/0701 device re-parses its GrAT/GrOAT on
//! LoadCompleted alone, with no restart — no capture of a table-only download
//! exists (ETS always rewrites the parameter image too).`

use bussard_mgmt::connection::{L4Channel, Layer4Connection};
use bussard_mgmt::load::{LoadControl, LoadState, WriteError, read_memory, write_memory};
use bussard_mgmt::sys7::{S7_SUB_ALLOC_DATA, encode_alloc_segment, encode_task_segment};
use bussard_mgmt::{LsmAccess, Sys7Profile, alloc_attr_octets};

use crate::compute::DesiredTables;
use crate::compute_sys7::{
    SYS7_ASSOCIATION_REGION_LEN, SYS7_GROUP_OBJECT_HEADER_LEN, sys7_address_table,
    sys7_association_table,
};
use crate::tables_sys7::{Sys7LiveTables, read_region};
use std::collections::BTreeSet;

/// The load-state machine that owns the `0x4000` address-table region
/// (`[system7-spec §3]`).
pub const SYS7_ADDRESS_LSM: u8 = 1;
/// The load-state machine that owns the `0x4201` association-table region
/// (`[system7-spec §3]`).
pub const SYS7_ASSOCIATION_LSM: u8 = 2;

/// Applying the System 7 tables failed.
#[derive(Debug, thiserror::Error)]
pub enum Sys7ApplyError {
    /// The computed image does not fit the memory region it must live in.
    #[error(
        "the computed System 7 {table} image is {len} octets but its region at {base:#06X} holds only {capacity} — refusing to write past the segment"
    )]
    RegionOverflow {
        /// Which region (`address`, `association`).
        table: &'static str,
        /// The region base.
        base: u16,
        /// The image length.
        len: usize,
        /// The region's capacity in octets.
        capacity: usize,
    },
    /// A written chunk did not read back as written.
    #[error(
        "the System 7 {table} image did not read back as written at {addr:#06X} (wrote {expected:02X?}, read {got:02X?})"
    )]
    VerifyMismatch {
        /// Which image.
        table: &'static str,
        /// The chunk address.
        addr: u16,
        /// What was written.
        expected: Vec<u8>,
        /// What the device answered.
        got: Vec<u8>,
    },
    /// An underlying management / load error.
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// The two region images `apply` will write, computed from the model and the
/// device's live group-object descriptors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sys7TableImages {
    /// Where the LSM 1 region starts on this device ([`Sys7LiveTables::address_base`]).
    pub address_base: u16,
    /// Where the LSM 2 region starts on this device
    /// ([`Sys7LiveTables::association_base`]).
    pub association_base: u16,
    /// The LSM 1 image: the new address table, nothing behind it.
    pub address_region: Vec<u8>,
    /// The `0x4201` association-table image.
    pub association_image: Vec<u8>,
    /// The communication-flag octets to write after the tables, in object order.
    pub flag_writes: Vec<Sys7FlagWrite>,
    /// Objects the model links anew whose communication flag could not be
    /// checked, because the group-object table's place is unknown (no product
    /// data). The device may ignore their new links.
    pub unchecked_flags: Vec<u16>,
}

/// One communication-flag change: the CONFIG octet of one object's descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sys7FlagWrite {
    /// The com-object number (ASAP).
    pub object: u16,
    /// The CONFIG octet's address in the group-object table.
    pub address: u16,
    /// The octet as read.
    pub from: u8,
    /// The octet to write.
    pub to: u8,
}

impl Sys7FlagWrite {
    /// Whether this write switches the communication flag on (else off).
    pub fn enables(&self) -> bool {
        self.to & SYS7_COMMUNICATION_FLAG != 0
    }
}

/// The communication-enable bit (C) of a descriptor's CONFIG octet
/// (`[system7-spec §7.3]`).
const SYS7_COMMUNICATION_FLAG: u8 = 0x04;

/// What the post-write verification found.
#[derive(Debug, Clone)]
pub struct Sys7VerifyOutcome {
    /// LSM 1's final load state (must be [`LoadState::Loaded`]).
    pub address_state: LoadState,
    /// LSM 2's final load state (must be [`LoadState::Loaded`]).
    pub association_state: LoadState,
    /// Whether the `0x4000` region read back byte-for-byte.
    pub addresses_match: bool,
    /// Whether the `0x4201` region read back byte-for-byte.
    pub associations_match: bool,
    /// Whether every communication-flag octet read back as written.
    pub flags_match: bool,
}

impl Sys7VerifyOutcome {
    /// The whole apply verified: both LSMs loaded and both regions byte-equal.
    pub fn ok(&self) -> bool {
        self.address_state == LoadState::Loaded
            && self.association_state == LoadState::Loaded
            && self.addresses_match
            && self.associations_match
            && self.flags_match
    }
}

/// Computes the two region images from the model's desired tables and the
/// device's live state.
///
/// `own_ia` goes into address-table slot 0 (TSAP 0, `[system7-spec §7.1]`) and is
/// the **address bussard is talking to**, not the value decoded off the device:
/// a System 7 download streams the product's static segment image, which carries
/// whatever own-IA slot the vendor shipped (commonly `0x0000`), and `A_Memory`
/// is the only thing that ever fixes it up. Writing the connected address is the
/// only value that can be right.
///
/// The address table is refused past [`Sys7LiveTables::address_capacity`], the
/// association table past its region. The communication flags follow the links
/// (see the module docs, [`sys7_flag_writes`]).
pub fn sys7_table_images(
    live: &Sys7LiveTables,
    desired: &DesiredTables,
    own_ia: u16,
) -> Result<Sys7TableImages, Sys7ApplyError> {
    let address_region = sys7_address_table(own_ia, desired);
    if address_region.len() > live.address_capacity {
        return Err(Sys7ApplyError::RegionOverflow {
            table: "address",
            base: live.address_base,
            len: address_region.len(),
            capacity: live.address_capacity,
        });
    }

    let association_image = sys7_association_table(desired);
    // A packed product (`DynamicTableManagement`, issue #383) keeps the
    // association table directly behind the address table, as ETS writes it:
    // it moves with the new address table's length and must end before the
    // product's LSM 2 segment does.
    let (association_base, association_capacity) = match live.packed_end {
        Some(end) => {
            let base = usize::from(live.address_base) + address_region.len();
            (
                u16::try_from(base).unwrap_or(u16::MAX),
                usize::from(end).saturating_sub(base),
            )
        }
        None => (live.association_base, SYS7_ASSOCIATION_REGION_LEN),
    };
    if association_image.len() > association_capacity {
        return Err(Sys7ApplyError::RegionOverflow {
            table: "association",
            base: association_base,
            len: association_image.len(),
            capacity: association_capacity,
        });
    }

    let linked: BTreeSet<u16> = desired.associations.iter().map(|&(_, a)| a).collect();
    let (flag_writes, unchecked_flags) = sys7_flag_writes(live, &linked);
    Ok(Sys7TableImages {
        address_base: live.address_base,
        association_base,
        address_region,
        association_image,
        flag_writes,
        unchecked_flags,
    })
}

/// The communication-flag writes that keep `live`'s group-object table in step
/// with the objects the model links (`linked`, see the module docs): C on for
/// every linked object that lacks it, C off for an object that loses its last
/// link. The second list names the objects that gain their first link while
/// the table is unknown, or that it does not describe.
pub fn sys7_flag_writes(
    live: &Sys7LiveTables,
    linked: &BTreeSet<u16>,
) -> (Vec<Sys7FlagWrite>, Vec<u16>) {
    let was_linked: BTreeSet<u16> = live
        .tables
        .associations
        .iter()
        .filter(|&&(tsap, _)| tsap != 0)
        .map(|&(_, a)| a)
        .collect();
    let Some(base) = live.group_object_base else {
        return (
            Vec::new(),
            linked.difference(&was_linked).copied().collect(),
        );
    };
    let mut writes = Vec::new();
    let mut unchecked = Vec::new();
    let mut want = |object: u16, on: bool| {
        let Some(descriptor) = live.group_objects.get(usize::from(object)) else {
            if on && !was_linked.contains(&object) {
                unchecked.push(object);
            }
            return;
        };
        let to = if on {
            descriptor.config | SYS7_COMMUNICATION_FLAG
        } else {
            descriptor.config & !SYS7_COMMUNICATION_FLAG
        };
        if to != descriptor.config {
            // [CNT:1][RAM-flags ptr:2], then 4-octet descriptors with CONFIG third.
            let offset = SYS7_GROUP_OBJECT_HEADER_LEN + 4 * usize::from(object) + 2;
            writes.push(Sys7FlagWrite {
                object,
                address: base.wrapping_add(offset as u16),
                from: descriptor.config,
                to,
            });
        }
    };
    for &object in linked {
        want(object, true);
    }
    for &object in was_linked.difference(linked) {
        want(object, false);
    }
    writes.sort_by_key(|w| w.object);
    (writes, unchecked)
}

/// Writes the two table regions to a System 7 device and verifies the result.
///
/// Runs the ordered sequence in the module docs on an **already authorized**
/// layer-4 connection (System 7 gates memory access behind `A_Authorize`), then
/// re-reads both regions and both LSM states. The caller treats
/// `!outcome.ok()` as a hard failure.
///
/// This is the only function here that mutates the device. It is exercised by the
/// mock-device tests and, in production, only from `bussard apply` after a plan
/// has been shown, confirmed and backed up.
pub async fn apply_sys7_tables<Ch: L4Channel>(
    l4: &mut Layer4Connection<Ch>,
    lsm: &LsmAccess,
    profile: &Sys7Profile,
    images: &Sys7TableImages,
    task_marker: [u8; 4],
) -> Result<Sys7VerifyOutcome, Sys7ApplyError> {
    // 0: negotiate the APDU size before any table write, as ETS does (#116).
    crate::apply::negotiate_session_apdu(l4).await?;

    // 1: tear both table LSMs down.
    lsm.drive(l4, SYS7_ASSOCIATION_LSM, LoadControl::Unload)
        .await?;
    lsm.drive(l4, SYS7_ADDRESS_LSM, LoadControl::Unload).await?;

    // 2: open both before either is written (the orphaned-TSAP guard).
    lsm.drive(l4, SYS7_ASSOCIATION_LSM, LoadControl::StartLoading)
        .await?;
    lsm.drive(l4, SYS7_ADDRESS_LSM, LoadControl::StartLoading)
        .await?;

    // 3: the LSM 1 region — allocate exactly what will be written, then stream.
    alloc_region(
        l4,
        lsm,
        profile,
        SYS7_ADDRESS_LSM,
        images.address_base,
        images.address_region.len(),
    )
    .await?;
    write_verified(l4, "address", images.address_base, &images.address_region).await?;

    // 4: the LSM 2 region — its TSAPs index the content just written.
    alloc_region(
        l4,
        lsm,
        profile,
        SYS7_ASSOCIATION_LSM,
        images.association_base,
        images.association_image.len(),
    )
    .await?;
    write_verified(
        l4,
        "association",
        images.association_base,
        &images.association_image,
    )
    .await?;

    // 5: finalize and activate the address table first.
    lsm.send_control(
        l4,
        SYS7_ADDRESS_LSM,
        &encode_task_segment(images.address_base, task_marker),
    )
    .await?;
    let address_state = lsm
        .drive(l4, SYS7_ADDRESS_LSM, LoadControl::LoadCompleted)
        .await?;

    // 6: then the association table.
    lsm.send_control(
        l4,
        SYS7_ASSOCIATION_LSM,
        &encode_task_segment(images.association_base, task_marker),
    )
    .await?;
    let association_state = lsm
        .drive(l4, SYS7_ASSOCIATION_LSM, LoadControl::LoadCompleted)
        .await?;

    // 7: the communication flags of the objects whose links changed.
    for flag in &images.flag_writes {
        write_verified(l4, "group-object", flag.address, &[flag.to]).await?;
    }

    // Verify by reading both regions and every flag octet back off the loaded
    // device.
    let addr_back = read_region(l4, images.address_base, images.address_region.len()).await?;
    let assoc_back =
        read_region(l4, images.association_base, images.association_image.len()).await?;
    let mut flags_match = true;
    for flag in &images.flag_writes {
        flags_match &= read_region(l4, flag.address, 1).await? == [flag.to];
    }

    Ok(Sys7VerifyOutcome {
        address_state,
        association_state,
        addresses_match: addr_back == images.address_region,
        associations_match: assoc_back == images.association_image,
        flags_match,
    })
}

/// Sends the absolute Data-segment allocation record for one table region
/// (`[system7-spec §4.2]`), sized to exactly the image about to be streamed.
///
/// Allocating the written span (rather than the whole region) keeps the device's
/// own bound check honest: nothing outside the image is claimed, so a later read
/// of a stale byte past the table is out of segment on a device that enforces it.
async fn alloc_region<Ch: L4Channel>(
    l4: &mut Layer4Connection<Ch>,
    lsm: &LsmAccess,
    profile: &Sys7Profile,
    index: u8,
    base: u16,
    len: usize,
) -> Result<(), WriteError> {
    let mem_type = profile.eeprom_mem_type;
    let (seg_flags, checksum_ctrl) = alloc_attr_octets(mem_type);
    let record = encode_alloc_segment(
        S7_SUB_ALLOC_DATA,
        base,
        len.min(usize::from(u16::MAX)) as u16,
        seg_flags,
        mem_type,
        checksum_ctrl,
    );
    lsm.send_control(l4, index, &record).await
}

/// Streams `image` to `base` in 12-octet `A_Memory_Write` chunks, reading each
/// chunk back and comparing before the next one goes out
/// (`[system7-spec §6]`: `A_Memory_Write` is unconfirmed, so read-back is the
/// only verification).
///
/// A differential link write is tiny (tens of octets), so the doubled exchange
/// count is cheap insurance against a device that silently drops or truncates a
/// chunk — and it fails at the offending chunk rather than after the whole table
/// has been streamed on top of it.
async fn write_verified<Ch: L4Channel>(
    l4: &mut Layer4Connection<Ch>,
    table: &'static str,
    base: u16,
    image: &[u8],
) -> Result<(), Sys7ApplyError> {
    let chunk = usize::from(l4.max_memory_chunk()).max(1);
    let mut offset = 0usize;
    while offset < image.len() {
        let take = chunk.min(image.len() - offset);
        let at = base.wrapping_add(offset as u16);
        let piece = &image[offset..offset + take];
        write_memory(l4, u32::from(at), piece).await?;
        let got = read_memory(l4, u32::from(at), take as u8).await?;
        if got != piece {
            return Err(Sys7ApplyError::VerifyMismatch {
                table,
                addr: at,
                expected: piece.to_vec(),
                got,
            });
        }
        offset += take;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::compute_tables;
    use crate::compute_sys7::{
        SYS7_ADDRESS_REGION_LEN, SYS7_ADDRESS_TABLE_ADDR, SYS7_ASSOCIATION_TABLE_ADDR,
        Sys7GroupObject,
    };
    use bussard_mgmt::tables::DeviceTables;
    use bussard_model::schema::Link;

    fn live(own_ia: u16, address_capacity: usize) -> Sys7LiveTables {
        Sys7LiveTables {
            tables: DeviceTables {
                mask: 0x0705,
                addresses: Vec::new(),
                associations: Vec::new(),
                resolved: Vec::new(),
                sources: Vec::new(),
                notes: Vec::new(),
                group_objects: None,
            },
            own_ia,
            address_capacity,
            address_base: SYS7_ADDRESS_TABLE_ADDR,
            association_base: SYS7_ASSOCIATION_TABLE_ADDR,
            packed_end: None,
            group_object_base: None,
            group_object_image: Vec::new(),
            group_objects: Vec::new(),
        }
    }

    /// The EK-E72-TP 1.1.39 as read live: five links (objects 20, 24, 27, 38
    /// and 52), its group-object table at `0x41D8`, the communication flag on
    /// exactly the linked objects (CONFIG `4F`) and off on the eight preset
    /// setpoints 29..=36 (CONFIG `DB`).
    fn e72() -> Sys7LiveTables {
        let mut live = live(0x1127, 237);
        live.association_base = 0x40ED;
        live.tables.associations = vec![(1, 20), (2, 24), (3, 27), (4, 38), (5, 52)];
        live.group_object_base = Some(0x41D8);
        live.group_objects = (0..=52)
            .map(|asap| Sys7GroupObject {
                asap,
                data_ptr: 0x0775 + asap,
                config: match asap {
                    20 | 24 | 27 | 38 | 52 => 0x4F,
                    29..=36 => 0xDB,
                    _ => 0x13,
                },
                type_code: 0x08,
            })
            .collect();
        live
    }

    fn objects(list: &[u16]) -> BTreeSet<u16> {
        list.iter().copied().collect()
    }

    fn desired_one_link() -> DesiredTables {
        compute_tables(&[Link {
            object: 1,
            name: None,
            send: Some("1/0/1".parse().expect("a GA")),
            listen: Vec::new(),
        }])
    }

    #[test]
    fn test_sys7_table_images_writes_only_the_address_table() {
        // The octets behind the address table are the unused rest of its
        // segment, never carried along: the image is the table and nothing more.
        let images =
            sys7_table_images(&live(0x1105, 237), &desired_one_link(), 0x1105).expect("images");
        assert_eq!(
            images.address_region,
            vec![0x02, 0x11, 0x05, 0x08, 0x01],
            "CNT + own IA + one GA, nothing behind it"
        );
        assert_eq!(images.association_image, vec![0x01, 0x01, 0x01]);
    }

    #[test]
    fn test_sys7_table_images_refuses_an_address_table_past_its_segment() {
        // A three-octet segment holds the own IA and no group address: refuse
        // rather than write into whatever follows it.
        let err = sys7_table_images(&live(0x1105, 3), &desired_one_link(), 0x1105)
            .expect_err("an address table past its segment is refused");
        assert!(
            matches!(
                err,
                Sys7ApplyError::RegionOverflow {
                    table: "address",
                    len: 5,
                    capacity: 3,
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn test_sys7_table_images_packs_the_association_table() -> Result<(), Sys7ApplyError> {
        // The Jung A-2088 (DynamicTableManagement, issue #383): the device's
        // association table sits right behind a 31-octet address table at
        // 0x401F; the model has one GA, so the address table is 5 octets and
        // the association table moves to 0x4005, inside 0x4000..0x43FE.
        let mut live = live(0x1131, SYS7_ADDRESS_REGION_LEN);
        live.association_base = 0x401F;
        live.packed_end = Some(0x43FE);
        let images = sys7_table_images(&live, &desired_one_link(), 0x1131)?;
        assert_eq!(images.address_base, 0x4000);
        assert_eq!(images.address_region.len(), 5);
        assert_eq!(images.association_base, 0x4005);
        assert_eq!(images.association_image, vec![0x01, 0x01, 0x01]);
        Ok(())
    }

    #[test]
    fn test_sys7_table_images_refuses_a_packed_table_past_the_segment() {
        // Both tables must end before the product's LSM 2 segment does
        // (M-00E1_A-A00D: 0x4000/23 + 0x4017/21, so they end at 0x402C).
        let mut live = live(0x1131, SYS7_ADDRESS_REGION_LEN);
        live.packed_end = Some(0x4007);
        let err = sys7_table_images(&live, &desired_one_link(), 0x1131);
        assert!(
            matches!(
                err,
                Err(Sys7ApplyError::RegionOverflow {
                    table: "association",
                    base: 0x4005,
                    capacity: 2,
                    ..
                })
            ),
            "{err:?}"
        );
    }

    #[test]
    fn test_sys7_flag_writes_switches_on_a_newly_linked_object() {
        // 1.1.39 links its standby cooling setpoint (object 32): the one CONFIG
        // octet at 0x41D8 + 3 + 4 * 32 + 2 goes DB → DF, as the live fix wrote.
        let (writes, unchecked) = sys7_flag_writes(&e72(), &objects(&[20, 24, 27, 32, 38, 52]));
        assert_eq!(
            writes,
            vec![Sys7FlagWrite {
                object: 32,
                address: 0x425D,
                from: 0xDB,
                to: 0xDF,
            }]
        );
        assert!(writes[0].enables());
        assert!(unchecked.is_empty());
    }

    #[test]
    fn test_sys7_flag_writes_repairs_a_linked_object_without_its_flag() {
        // The state an older apply left: object 32 linked, its flag still off.
        let mut live = e72();
        live.tables.associations.push((6, 32));
        let (writes, _) = sys7_flag_writes(&live, &objects(&[20, 24, 27, 32, 38, 52]));
        assert_eq!(writes.len(), 1);
        assert_eq!((writes[0].object, writes[0].to), (32, 0xDF));
    }

    #[test]
    fn test_sys7_flag_writes_switches_off_an_object_losing_its_last_link() {
        // Fan speed (object 52) loses its only link: 4F → 4B. The others keep
        // theirs and are left alone.
        let (writes, _) = sys7_flag_writes(&e72(), &objects(&[20, 24, 27, 38]));
        assert_eq!(
            writes,
            vec![Sys7FlagWrite {
                object: 52,
                address: 0x41D8 + 3 + 4 * 52 + 2,
                from: 0x4F,
                to: 0x4B,
            }]
        );
        assert!(!writes[0].enables());
    }

    #[test]
    fn test_sys7_flag_writes_leaves_a_matching_device_alone() {
        let (writes, unchecked) = sys7_flag_writes(&e72(), &objects(&[20, 24, 27, 38, 52]));
        assert!(writes.is_empty());
        assert!(unchecked.is_empty());
    }

    #[test]
    fn test_sys7_flag_writes_names_new_links_it_cannot_check() {
        // Without product data the group-object table's place is unknown: no
        // write, and the newly linked object is named, the already linked not.
        let mut live = e72();
        live.group_object_base = None;
        live.group_objects.clear();
        let (writes, unchecked) = sys7_flag_writes(&live, &objects(&[20, 24, 27, 32, 38, 52]));
        assert!(writes.is_empty());
        assert_eq!(unchecked, vec![32]);
        // An object past the table the product declares cannot be checked either.
        let (writes, unchecked) = sys7_flag_writes(&e72(), &objects(&[20, 24, 27, 38, 52, 60]));
        assert!(writes.is_empty());
        assert_eq!(unchecked, vec![60]);
    }

    #[test]
    fn test_verify_outcome_requires_everything() {
        let base = Sys7VerifyOutcome {
            address_state: LoadState::Loaded,
            association_state: LoadState::Loaded,
            addresses_match: true,
            associations_match: true,
            flags_match: true,
        };
        assert!(base.ok());
        let mut bad = base.clone();
        bad.association_state = LoadState::Error;
        assert!(!bad.ok());
        let mut bad = base.clone();
        bad.addresses_match = false;
        assert!(!bad.ok());
        let mut bad = base.clone();
        bad.flags_match = false;
        assert!(!bad.ok());
    }
}
