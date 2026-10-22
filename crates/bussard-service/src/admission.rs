//! Secure sender admission (issue #297): whether the devices on a secured
//! group address accept a secured telegram from bussard's tunnel address, and
//! how `plan`/`apply` admit the tunnelling addresses.
//!
//! A KNX Data Secure device accepts a secured group telegram only from an
//! individual address in its security individual address table (PID 54,
//! `docs/knx-secure-spec.md` §5.7.1 and §11.1). ETS fills that table with the
//! devices that send on the secured group addresses the device listens to,
//! and with an interface's tunnelling address only when group addresses are
//! assigned to it. A tunnel address the table does not list is dropped
//! without an answer, so a secured `GroupValueWrite` the gateway confirmed
//! (L_Data.con) may still have done nothing.
//!
//! - [`check_sender_admission`] decides, before a secured write or read is
//!   sent, from the evidence bussard holds: the table recorded in the lock
//!   (`secure_senders`, read back by `bussard adopt`) and the keyring's or
//!   key store's interface entries ([`AdmissionEvidence`]). The verdict is
//!   [`Admission::Admitted`], [`Admission::Refused`] (refuse before sending,
//!   [`Admission::refusal`]) or [`Admission::Unknown`] (send, and say so,
//!   [`Admission::note`]).
//! - [`TunnelAdmission`] is the plan side of `[connection]
//!   admit_tunnel_senders = true`: the tunnelling addresses a Secure device's
//!   PID 54 is missing, as one plan change.
//!
//! Nothing here reads or prints key material: only addresses.

use std::collections::{BTreeMap, BTreeSet};

use bussard_download::{
    ChangeMark, ChangeSubject, DevicePlan, PlanChange, PlanReport, SecureSenderEntry,
};
use bussard_model::{GroupAddress, IndividualAddress, Model};

/// What `knx_write_group` and `bussard write` say a sent group write
/// achieved: the interface's confirmation, nothing more. A group write has no
/// application-level reply in KNX.
pub const WRITE_RESULT: &str = "the interface acknowledged the telegram (L_Data.con); KNX reports \
                                no application-level result";

/// What a sent group write achieved when the interface's L_Data.con was not
/// seen within the confirmation window.
pub const WRITE_RESULT_UNCONFIRMED: &str = "sent, but the interface's L_Data.con was not seen \
                                            within the confirmation window; KNX reports no \
                                            application-level result";

/// One interface entry of a keyring or key store, as admission evidence: its
/// address and the group addresses ETS assigned to it, each with the parsed
/// `Senders` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceGroups {
    /// The interface's individual address (the tunnelling address of a
    /// tunnelling user).
    pub ia: IndividualAddress,
    /// Whether this is a tunnelling interface (`Type="Tunneling"`).
    pub tunnelling: bool,
    /// The group addresses assigned to this interface address in ETS (the
    /// `<Group>` children), each with the individual addresses of its
    /// `Senders` attribute: the senders the interface itself accepts on it.
    pub groups: BTreeMap<GroupAddress, Vec<IndividualAddress>>,
}

/// The keyring's (or key store's) interface entries, the admission evidence
/// besides the lock (issue #297).
///
/// An interface `<Group Address=…>` is what ETS writes when the group
/// address is assigned to that tunnelling address; ETS then also lists the
/// tunnelling address in the PID 54 of the devices on that group address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdmissionEvidence {
    /// The interfaces, in keyring order.
    pub interfaces: Vec<InterfaceGroups>,
}

impl AdmissionEvidence {
    /// The interface entries of a decrypted keyring (a key store is served in
    /// the same shape). Reads addresses only.
    pub fn from_keyring(keyring: &bussard_project::Keyring) -> AdmissionEvidence {
        let interfaces = keyring
            .interfaces
            .iter()
            .map(|i| InterfaceGroups {
                ia: i.ia,
                tunnelling: i.interface_type.eq_ignore_ascii_case("Tunneling"),
                groups: i
                    .gas
                    .iter()
                    .map(|ga| {
                        let senders = i
                            .group_senders
                            .get(ga)
                            .map(|raw| parse_senders(raw))
                            .unwrap_or_default();
                        (*ga, senders)
                    })
                    .collect(),
            })
            .collect();
        AdmissionEvidence { interfaces }
    }

    /// The tunnelling addresses of the interfaces, ascending and without
    /// duplicates: the addresses `admit_tunnel_senders` admits. Every one is
    /// admitted because bussard picks the tunnelling user per connection
    /// (issue #288).
    pub fn tunnel_addresses(&self) -> Vec<IndividualAddress> {
        self.interfaces
            .iter()
            .filter(|i| i.tunnelling)
            .map(|i| i.ia)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Whether ETS assigned `ga` to the interface address `sender` (an
    /// interface entry for `sender` lists `ga`).
    pub fn assigned(&self, sender: IndividualAddress, ga: GroupAddress) -> bool {
        self.interfaces
            .iter()
            .any(|i| i.ia == sender && i.groups.contains_key(&ga))
    }
}

/// Parses an ETS `Senders` attribute: individual addresses separated by
/// whitespace (or commas). A token that is not an individual address is
/// skipped.
pub fn parse_senders(raw: &str) -> Vec<IndividualAddress> {
    raw.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect()
}

/// What the evidence says about one device on the group address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverVerdict {
    /// The device's recorded table lists the sender, or ETS assigned the group
    /// address to the sender's interface entry.
    Admitted,
    /// The device's recorded table (the lock's `secure_senders`) does not list
    /// the sender.
    NotListed,
    /// No recorded table and no ETS assignment: bussard cannot tell.
    Unknown,
}

/// The verdict of [`check_sender_admission`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// At least one device on the group address admits the sender.
    Admitted {
        /// The sender (the tunnel address).
        sender: IndividualAddress,
        /// The group address.
        ga: GroupAddress,
        /// The devices that admit it.
        receivers: Vec<IndividualAddress>,
        /// Devices whose recorded table does not list the sender.
        not_listed: Vec<IndividualAddress>,
    },
    /// Every device on the group address has a recorded table and none lists
    /// the sender: they would discard the telegram. Refuse before sending.
    Refused {
        /// The sender (the tunnel address).
        sender: IndividualAddress,
        /// The group address.
        ga: GroupAddress,
        /// The device the refusal names (the first, ascending).
        receiver: IndividualAddress,
        /// Every device on the group address.
        receivers: Vec<IndividualAddress>,
    },
    /// No evidence either way (no read-back yet, no ETS assignment): send,
    /// and say that delivery is not confirmed.
    Unknown {
        /// The sender (the tunnel address).
        sender: IndividualAddress,
        /// The group address.
        ga: GroupAddress,
        /// The devices bussard cannot tell about; empty when the model links
        /// no device to the group address.
        receivers: Vec<IndividualAddress>,
    },
}

impl Admission {
    /// `admitted`, `refused` or `unknown`, for a JSON field.
    pub fn label(&self) -> &'static str {
        match self {
            Admission::Admitted { .. } => "admitted",
            Admission::Refused { .. } => "refused",
            Admission::Unknown { .. } => "unknown",
        }
    }

    /// Whether the telegram must not be sent.
    pub fn is_refused(&self) -> bool {
        matches!(self, Admission::Refused { .. })
    }

    /// The refusal sentence, for [`Admission::Refused`]:
    /// `1.1.5 does not list the tunnel address 1.1.25 as a secure sender of
    /// 1/0/4; …`.
    pub fn refusal(&self) -> Option<String> {
        let Admission::Refused {
            sender,
            ga,
            receiver,
            ..
        } = self
        else {
            return None;
        };
        Some(format!(
            "{receiver} does not list the tunnel address {sender} as a secure sender of {ga}; \
             the actuator would discard the telegram. Admit the tunnel addresses (see below) \
             or send plain only if the GA is not secured on the device."
        ))
    }

    /// How to admit the tunnel addresses, the "see below" of
    /// [`refusal`](Self::refusal).
    pub fn admit_hint(&self) -> Option<String> {
        let Admission::Refused {
            ga,
            receiver,
            receivers,
            ..
        } = self
        else {
            return None;
        };
        let devices: Vec<String> = receivers.iter().map(ToString::to_string).collect();
        Some(format!(
            "To admit the tunnel addresses: set `admit_tunnel_senders = true` under \
             [connection] in bussard.toml, then plan and apply {} (`bussard apply {receiver}`, \
             or knx_plan_device and knx_apply_device), which adds every tunnelling address \
             of the key store to its security individual address table (PID 54); or assign \
             {ga} to the tunnelling addresses in ETS and download the device, then `bussard \
             adopt {receiver}` to record the table ETS wrote.",
            devices.join(", ")
        ))
    }

    /// The note of a telegram sent without evidence, for
    /// [`Admission::Unknown`]: `sent secured from 1.1.25; delivery to 1.1.5
    /// is not confirmed by KNX; …`.
    pub fn note(&self) -> Option<String> {
        let Admission::Unknown {
            sender,
            ga,
            receivers,
        } = self
        else {
            return None;
        };
        let (who, verb) = match receivers.as_slice() {
            [] => (format!("the receivers of {ga}"), "do"),
            [one] => (one.to_string(), "does"),
            many => (
                many.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                "do",
            ),
        };
        Some(format!(
            "sent secured from {sender}; delivery to {who} is not confirmed by KNX; if nothing \
             happens, {who} probably {verb} not admit {sender}"
        ))
    }
}

/// The devices the model links to `ga` (listening or sending on it), other
/// than `sender`, ascending: the receivers of a write and the responders of a
/// read.
pub fn linked_devices(
    model: Option<&Model>,
    ga: GroupAddress,
    sender: IndividualAddress,
) -> Vec<IndividualAddress> {
    let Some(model) = model else {
        return Vec::new();
    };
    model
        .links
        .links
        .iter()
        .filter(|(ia, links)| {
            **ia != sender
                && links
                    .iter()
                    .any(|l| l.send == Some(ga) || l.listen.contains(&ga))
        })
        .map(|(ia, _)| *ia)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// What the evidence says about `device` admitting `sender` on `ga`.
///
/// The recorded table wins: a lock `secure_senders` list (read back from the
/// device) admits exactly what it lists. Without one, an ETS assignment of
/// `ga` to the sender's interface entry admits it. Otherwise it is unknown.
pub fn receiver_verdict(
    model: Option<&Model>,
    device: IndividualAddress,
    ga: GroupAddress,
    sender: IndividualAddress,
    evidence: &AdmissionEvidence,
) -> ReceiverVerdict {
    let recorded = model
        .and_then(|m| m.devices.get(&device))
        .and_then(|d| d.device.security.as_ref())
        .map(|s| s.secure_senders.as_slice())
        .unwrap_or(&[]);
    if !recorded.is_empty() {
        return if recorded.iter().any(|s| s.address == sender) {
            ReceiverVerdict::Admitted
        } else {
            ReceiverVerdict::NotListed
        };
    }
    if evidence.assigned(sender, ga) {
        ReceiverVerdict::Admitted
    } else {
        ReceiverVerdict::Unknown
    }
}

/// Whether the devices on the secured `ga` admit `sender` (issue #297).
///
/// - [`Admission::Admitted`] when at least one device on `ga` admits it;
/// - [`Admission::Refused`] when the model links devices to `ga`, each has a
///   recorded table, and none lists `sender`;
/// - [`Admission::Unknown`] otherwise.
///
/// Pure: the caller passes the tunnel's individual address once the tunnel
/// is up, and only calls this for a secured group address.
pub fn check_sender_admission(
    model: Option<&Model>,
    ga: GroupAddress,
    sender: IndividualAddress,
    evidence: &AdmissionEvidence,
) -> Admission {
    let devices = linked_devices(model, ga, sender);
    let mut admitted = Vec::new();
    let mut not_listed = Vec::new();
    let mut unknown = Vec::new();
    for device in &devices {
        match receiver_verdict(model, *device, ga, sender, evidence) {
            ReceiverVerdict::Admitted => admitted.push(*device),
            ReceiverVerdict::NotListed => not_listed.push(*device),
            ReceiverVerdict::Unknown => unknown.push(*device),
        }
    }
    if !admitted.is_empty() {
        return Admission::Admitted {
            sender,
            ga,
            receivers: admitted,
            not_listed,
        };
    }
    match (not_listed.first(), unknown.is_empty()) {
        (Some(first), true) => Admission::Refused {
            sender,
            ga,
            receiver: *first,
            receivers: devices,
        },
        _ => {
            // Without any linked device the evidence is empty, not negative.
            Admission::Unknown {
                sender,
                ga,
                receivers: unknown,
            }
        }
    }
}

/// The reason of a secured read that timed out (issue #297): on a secured
/// group address the usual cause is a responder that does not admit the
/// sender.
pub fn secured_read_timeout(ga: GroupAddress, sender: IndividualAddress) -> String {
    format!(
        "no verified secured response for {ga} within the timeout; on a secured group address \
         a timeout usually means the responder does not admit the sender {sender} (its \
         security individual address table, PID 54, does not list it)"
    )
}

/// Whether `bussard.toml` turns on `[connection] admit_tunnel_senders`.
pub fn admit_tunnel_senders(model: Option<&Model>) -> bool {
    model.is_some_and(|m| m.config.connection.admit_tunnel_senders)
}

/// The plan side of `admit_tunnel_senders` for one Secure device (issue
/// #297): the tunnelling addresses to admit and which of them the device's
/// PID 54 is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelAdmission {
    /// The device.
    pub device: IndividualAddress,
    /// Every tunnelling address the security object will list, ascending
    /// (the device itself excluded).
    pub tunnel: Vec<IndividualAddress>,
    /// The tunnelling addresses the device's PID 54 does not list, or all of
    /// them when the table could not be read.
    pub missing: Vec<IndividualAddress>,
    /// Whether PID 54 was read (`false`: the device refused the read).
    pub table_read: bool,
}

impl TunnelAdmission {
    /// Compares the tunnelling addresses with the device's PID 54 as read
    /// (`None`: the read was refused, so every address counts as missing).
    pub fn new(
        device: IndividualAddress,
        tunnel: &[IndividualAddress],
        live: Option<&[SecureSenderEntry]>,
    ) -> TunnelAdmission {
        let tunnel: Vec<IndividualAddress> = tunnel
            .iter()
            .copied()
            .filter(|ia| *ia != device)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let missing = match live {
            Some(entries) => tunnel
                .iter()
                .copied()
                .filter(|ia| !entries.iter().any(|e| e.address == *ia))
                .collect(),
            None => tunnel.clone(),
        };
        TunnelAdmission {
            device,
            tunnel,
            missing,
            table_read: live.is_some(),
        }
    }

    /// Whether the security object must be written for the admission.
    pub fn changes(&self) -> bool {
        !self.missing.is_empty()
    }

    /// The plan line: `+ the security individual address table (PID 54)
    /// admits the tunnelling addresses 1.1.22, 1.1.23 as secure senders
    /// (sequence 0)`, or `None` when nothing is missing.
    pub fn change(&self) -> Option<PlanChange> {
        if !self.changes() {
            return None;
        }
        let list: Vec<String> = self.missing.iter().map(ToString::to_string).collect();
        let unread = if self.table_read {
            String::new()
        } else {
            "; the table could not be read, so it is written".to_string()
        };
        Some(PlanChange {
            mark: ChangeMark::Added,
            subject: ChangeSubject::Security,
            channel: None,
            key: "security individual address table".to_string(),
            object: None,
            sentence: format!(
                "the security individual address table (PID 54) admits the tunnelling \
                 address{} {} as secure sender{} (sequence 0, admit_tunnel_senders){unread}",
                if self.missing.len() == 1 { "" } else { "es" },
                list.join(", "),
                if self.missing.len() == 1 { "" } else { "s" },
            ),
        })
    }

    /// Adds the change to `plan`: the line, and the writes (the security
    /// object, and the tables it is written inside of). Does nothing when
    /// nothing is missing. The state hash takes in the table as read, so an
    /// approved plan is refused if PID 54 changes before the apply.
    pub fn add_to(&self, plan: &mut DevicePlan, report: &PlanReport) {
        let Some(change) = self.change() else {
            return;
        };
        plan.changes.insert(0, change);
        plan.writes.security_object = true;
        plan.writes
            .address_table
            .get_or_insert(report.resulting_address_count);
        plan.writes
            .association_table
            .get_or_insert(report.resulting_association_count);
        let missing: Vec<String> = self.missing.iter().map(ToString::to_string).collect();
        plan.state_hash = format!(
            "{}-pid54-{}",
            plan.state_hash,
            short_digest(&missing.join(","))
        );
    }
}

/// Why [`record_secure_senders`] could not update the lock.
#[derive(Debug, thiserror::Error)]
pub enum RecordSendersError {
    /// The model did not load.
    #[error("loading the model to record the secure senders: {0}")]
    Load(#[from] bussard_model::LoadError),
    /// The model did not save.
    #[error("saving the secure senders to bussard.lock: {0}")]
    Save(#[from] bussard_model::SaveError),
}

/// Records the security individual address table a verified secured write
/// put on `device` (PID 54) as its lock `secure_senders`, the shape `bussard
/// adopt` writes (issue #297). The next secured write to a GA of the device
/// then sees the table and reports `admitted` without an `adopt`.
///
/// Call it only after the write verified. Loads the model from `dir`, changes
/// only that device's `secure_senders` and saves (files whose contents do not
/// change are not rewritten). Returns whether the lock changed; a device the
/// model does not know is left alone (`false`).
///
/// # Errors
///
/// The model does not load or save.
pub fn record_secure_senders(
    dir: &std::path::Path,
    device: IndividualAddress,
    entries: &[SecureSenderEntry],
) -> Result<bool, RecordSendersError> {
    let mut model = Model::load(dir)?;
    let Some(loaded) = model.devices.get_mut(&device) else {
        return Ok(false);
    };
    let senders: Vec<bussard_model::schema::SecureSender> = entries
        .iter()
        .map(|e| bussard_model::schema::SecureSender {
            address: e.address,
            sequence: e.sequence,
        })
        .collect();
    let security = loaded.device.security.get_or_insert_with(Default::default);
    if security.secure_senders == senders {
        return Ok(false);
    }
    security.secure_senders = senders;
    model.save(dir)?;
    Ok(true)
}

/// A short, stable digest of `text` (FNV-1a, 64 bits, hex), for the state
/// hash suffix.
fn short_digest(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bussard_model::schema::SecureSender;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn ia(s: &str) -> Result<IndividualAddress, Box<dyn std::error::Error>> {
        Ok(s.parse()?)
    }

    fn ga(s: &str) -> Result<GroupAddress, Box<dyn std::error::Error>> {
        Ok(s.parse()?)
    }

    /// The fixture: 1.1.5 listens on the secured 1/0/4 and sends 1/0/5;
    /// `senders` is its lock `secure_senders`.
    fn model(senders: &[(&str, u64)]) -> Result<Model, Box<dyn std::error::Error>> {
        use bussard_model::schema::{
            BussardConfig, Device, DeviceSecurity, Group, Groups, Link, Links,
        };
        let mut groups = Groups::default();
        groups.groups.insert(
            ga("1/0/4")?,
            Group {
                name: "Licht".into(),
                secure: true,
                ..Default::default()
            },
        );
        let mut links = Links::default();
        links.links.insert(
            ia("1.1.5")?,
            vec![
                Link {
                    object: 1,
                    name: None,
                    send: None,
                    listen: vec![ga("1/0/4")?],
                },
                Link {
                    object: 2,
                    name: None,
                    send: Some(ga("1/0/5")?),
                    listen: Vec::new(),
                },
            ],
        );
        let mut security = DeviceSecurity {
            activated: true,
            ..Default::default()
        };
        for (address, sequence) in senders {
            security.secure_senders.push(SecureSender {
                address: ia(address)?,
                sequence: *sequence,
            });
        }
        let device = Device {
            address: ia("1.1.5")?,
            name: "Aktor".to_string(),
            description: None,
            location: None,
            product: None,
            channels: Default::default(),
            parameters: Default::default(),
            module_bases: Default::default(),
            com_objects: Default::default(),
            security: Some(security),
            replaced: None,
            application_override: None,
            lock: Default::default(),
        };
        let mut devices = BTreeMap::new();
        devices.insert(
            device.address,
            bussard_model::LoadedDevice {
                device,
                file_stem: "1.1.5".to_string(),
            },
        );
        Ok(Model {
            config: BussardConfig::default(),
            groups,
            links,
            devices,
        })
    }

    fn evidence(ia_: &str, gas: &[&str]) -> Result<AdmissionEvidence, Box<dyn std::error::Error>> {
        let mut groups = BTreeMap::new();
        for g in gas {
            groups.insert(ga(g)?, Vec::new());
        }
        Ok(AdmissionEvidence {
            interfaces: vec![InterfaceGroups {
                ia: ia(ia_)?,
                tunnelling: true,
                groups,
            }],
        })
    }

    #[test]
    fn test_check_sender_admission_refuses_when_the_lock_table_lacks_the_sender() -> TestResult {
        let model = model(&[("1.1.16", 0)])?;
        let verdict = check_sender_admission(
            Some(&model),
            ga("1/0/4")?,
            ia("1.1.25")?,
            &AdmissionEvidence::default(),
        );
        assert!(verdict.is_refused(), "{verdict:?}");
        assert_eq!(
            verdict.refusal().as_deref(),
            Some(
                "1.1.5 does not list the tunnel address 1.1.25 as a secure sender of 1/0/4; \
                 the actuator would discard the telegram. Admit the tunnel addresses (see \
                 below) or send plain only if the GA is not secured on the device."
            )
        );
        let hint = verdict.admit_hint().ok_or("no hint")?;
        assert!(hint.contains("admit_tunnel_senders = true"), "{hint}");
        assert!(verdict.note().is_none());
        Ok(())
    }

    #[test]
    fn test_check_sender_admission_admits_a_listed_sender() -> TestResult {
        let model = model(&[("1.1.16", 0), ("1.1.25", 0)])?;
        let verdict = check_sender_admission(
            Some(&model),
            ga("1/0/4")?,
            ia("1.1.25")?,
            &AdmissionEvidence::default(),
        );
        assert_eq!(verdict.label(), "admitted", "{verdict:?}");
        assert!(verdict.refusal().is_none() && verdict.note().is_none());
        Ok(())
    }

    #[test]
    fn test_check_sender_admission_is_unknown_without_a_recorded_table() -> TestResult {
        let model = model(&[])?;
        let verdict = check_sender_admission(
            Some(&model),
            ga("1/0/4")?,
            ia("1.1.25")?,
            &AdmissionEvidence::default(),
        );
        assert_eq!(verdict.label(), "unknown", "{verdict:?}");
        assert_eq!(
            verdict.note().as_deref(),
            Some(
                "sent secured from 1.1.25; delivery to 1.1.5 is not confirmed by KNX; if \
                 nothing happens, 1.1.5 probably does not admit 1.1.25"
            )
        );
        // The read side: the status 1/0/5 is sent by 1.1.5, its responder.
        let read = check_sender_admission(
            Some(&model),
            ga("1/0/5")?,
            ia("1.1.25")?,
            &AdmissionEvidence::default(),
        );
        assert_eq!(read.label(), "unknown", "{read:?}");
        // No model: no receivers to name.
        let none = check_sender_admission(
            None,
            ga("1/0/4")?,
            ia("1.1.25")?,
            &AdmissionEvidence::default(),
        );
        let note = none.note().ok_or("no note")?;
        assert!(note.contains("the receivers of 1/0/4"), "{note}");
        Ok(())
    }

    #[test]
    fn test_check_sender_admission_accepts_an_ets_interface_assignment() -> TestResult {
        let model = model(&[])?;
        let assigned = evidence("1.1.25", &["1/0/4"])?;
        let verdict = check_sender_admission(Some(&model), ga("1/0/4")?, ia("1.1.25")?, &assigned);
        assert_eq!(verdict.label(), "admitted", "{verdict:?}");
        // Another tunnel user is not covered by 1.1.25's assignment.
        let other = check_sender_admission(Some(&model), ga("1/0/4")?, ia("1.1.26")?, &assigned);
        assert_eq!(other.label(), "unknown", "{other:?}");
        // A read-back wins over the keyring.
        let recorded = model_with_lock()?;
        let verdict =
            check_sender_admission(Some(&recorded), ga("1/0/4")?, ia("1.1.25")?, &assigned);
        assert!(verdict.is_refused(), "{verdict:?}");
        Ok(())
    }

    fn model_with_lock() -> Result<Model, Box<dyn std::error::Error>> {
        model(&[("1.1.16", 0)])
    }

    #[test]
    fn test_admitted_tunnel_pid54_image_is_ascending() -> TestResult {
        // 1.1.16 sends on 1/0/4 (a link-derived sender); the key store has
        // tunnel users 1.1.29, 1.1.22 and 1.1.25 in that order.
        let mut model = model(&[])?;
        model.links.links.insert(
            ia("1.1.16")?,
            vec![bussard_model::schema::Link {
                object: 1,
                name: None,
                send: Some(ga("1/0/4")?),
                listen: Vec::new(),
            }],
        );
        let mut evidence = AdmissionEvidence::default();
        for address in ["1.1.29", "1.1.22", "1.1.25"] {
            evidence.interfaces.push(InterfaceGroups {
                ia: ia(address)?,
                tunnelling: true,
                groups: BTreeMap::new(),
            });
        }
        // A USB interface is not a tunnelling address.
        evidence.interfaces.push(InterfaceGroups {
            ia: ia("1.1.40")?,
            tunnelling: false,
            groups: BTreeMap::new(),
        });
        let tunnel = evidence.tunnel_addresses();
        let keys = std::collections::HashMap::new();
        let none = std::collections::HashMap::new();
        let on =
            bussard_download::secured_senders(Some(&model), ia("1.1.5")?, &keys, &none, &tunnel);
        let addresses: Vec<IndividualAddress> = on.iter().map(|e| e.address).collect();
        assert_eq!(
            addresses,
            vec![ia("1.1.16")?, ia("1.1.22")?, ia("1.1.25")?, ia("1.1.29")?]
        );
        assert!(on.iter().skip(1).all(|e| e.sequence == 0));
        // The image the device receives from element 1: ascending IA, 8 octets each.
        let image = bussard_download::sender_table_bytes(&on);
        assert_eq!(image.len(), 32);
        assert_eq!(image.get(..2), Some(&[0x11, 0x10][..]));
        assert_eq!(image.get(8..10), Some(&[0x11, 0x16][..]));
        assert_eq!(image.get(16..18), Some(&[0x11, 0x19][..]));
        assert_eq!(image.get(24..26), Some(&[0x11, 0x1D][..]));
        // Off: exactly the link-derived table (the offline oracle rows).
        let off = bussard_download::secured_senders(Some(&model), ia("1.1.5")?, &keys, &none, &[]);
        assert_eq!(off.len(), 1);
        assert_eq!(off.first().map(|e| e.address), Some(ia("1.1.16")?));
        Ok(())
    }

    #[test]
    fn test_keystore_import_keeps_interface_senders_as_evidence() -> TestResult {
        const PASSWORD: &str = "synthetic-keyring-pw";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../knx-sim/examples/secure/synthetic.knxkeys");
        let keyring = bussard_project::parse_keyring(&std::fs::read_to_string(path)?, PASSWORD)?;
        // Import into a key store, save and load it again: the shape every
        // command reads the store in.
        let mut store = bussard_project::KeyStore::new("Synthetic");
        store.merge_keyring(&keyring);
        let reloaded = bussard_project::KeyStore::parse(&store.to_xml(PASSWORD)?, PASSWORD)?;
        let evidence = AdmissionEvidence::from_keyring(&reloaded.to_keyring());
        assert_eq!(evidence.tunnel_addresses(), vec![ia("1.1.200")?]);
        let tunnel = evidence.interfaces.first().ok_or("no interface")?;
        assert_eq!(
            tunnel.groups.get(&ga("1/2/3")?),
            Some(&vec![ia("1.1.10")?]),
            "the interface's Senders survive the import"
        );
        assert!(evidence.assigned(ia("1.1.200")?, ga("1/2/3")?));
        assert!(!evidence.assigned(ia("1.1.255")?, ga("1/2/3")?));
        Ok(())
    }

    #[test]
    fn test_parse_senders_reads_ets_lists() -> TestResult {
        assert_eq!(
            parse_senders("1.1.10 1.1.16"),
            vec![ia("1.1.10")?, ia("1.1.16")?]
        );
        assert_eq!(
            parse_senders("1.1.10,1.1.16"),
            vec![ia("1.1.10")?, ia("1.1.16")?]
        );
        assert_eq!(parse_senders(" bogus 1.1.10 "), vec![ia("1.1.10")?]);
        assert!(parse_senders("").is_empty());
        Ok(())
    }

    #[test]
    fn test_tunnel_admission_lists_only_the_missing_addresses() -> TestResult {
        let tunnel = [ia("1.1.23")?, ia("1.1.22")?, ia("1.1.5")?];
        let live = [SecureSenderEntry {
            address: ia("1.1.22")?,
            sequence: 0,
        }];
        let admission = TunnelAdmission::new(ia("1.1.5")?, &tunnel, Some(&live));
        assert_eq!(admission.tunnel, vec![ia("1.1.22")?, ia("1.1.23")?]);
        assert_eq!(admission.missing, vec![ia("1.1.23")?]);
        let change = admission.change().ok_or("no change")?;
        assert_eq!(change.subject, ChangeSubject::Security);
        assert!(
            change
                .sentence
                .contains("tunnelling address 1.1.23 as secure sender")
        );
        // All present: no change.
        let all = [
            live[0],
            SecureSenderEntry {
                address: ia("1.1.23")?,
                sequence: 9,
            },
        ];
        assert!(
            TunnelAdmission::new(ia("1.1.5")?, &tunnel, Some(&all))
                .change()
                .is_none()
        );
        // Unreadable: every address is written.
        let unread = TunnelAdmission::new(ia("1.1.5")?, &tunnel, None);
        assert_eq!(unread.missing.len(), 2);
        Ok(())
    }
}
