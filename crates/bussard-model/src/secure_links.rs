//! Data Secure link checks (issue #330): a link that puts a device without
//! KNX Data Secure on a group address the other side runs secured.
//!
//! * **E035**: a device that cannot run Data Secure sends on a group address
//!   that a linked receiver takes secured. The receiver drops every plaintext
//!   telegram, so the link looks right and does nothing.
//! * **E036**: the reverse. A Secure device sends secured on a group address
//!   that a device without Data Secure listens to, and that receiver cannot
//!   decrypt what it gets.
//!
//! The checks read the model only (the group `secure` flag, the com object
//! `secure` flag, the device's `secure_capable` flag), never a key.

use std::collections::BTreeMap;

use crate::address::{GroupAddress, IndividualAddress};
use crate::loader::Model;
use crate::validate::{Diagnostic, Severity};

/// Which side of a link cannot run Data Secure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictKind {
    /// E035: the plain device sends, secured receivers drop its telegrams.
    PlainSender,
    /// E036: a Secure device sends secured, the plain device listens and
    /// cannot decrypt.
    PlainReceiver,
}

/// One link that puts a device without Data Secure on a group address the
/// other side runs secured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureLinkConflict {
    /// Which side is plain.
    pub kind: ConflictKind,
    /// The device without Data Secure.
    pub device: IndividualAddress,
    /// The plain device's com object on the link.
    pub object: u16,
    /// The index of that link in the device's link list (for the location).
    pub index: usize,
    /// The group address.
    pub ga: GroupAddress,
    /// The objects on the other side that run the GA secured: the secured
    /// receivers for [`ConflictKind::PlainSender`], the secured senders for
    /// [`ConflictKind::PlainReceiver`].
    pub secured: Vec<(IndividualAddress, u16)>,
}

impl SecureLinkConflict {
    /// The rule code: `E035` or `E036`.
    pub fn code(&self) -> &'static str {
        match self.kind {
            ConflictKind::PlainSender => "E035",
            ConflictKind::PlainReceiver => "E036",
        }
    }

    /// The sentence a human reads: who, on which GA, against whom, and the
    /// ways out.
    pub fn message(&self) -> String {
        let others: Vec<String> = self
            .secured
            .iter()
            .map(|(ia, object)| format!("{ia} object {object}"))
            .collect();
        let others = others.join(", ");
        let (device, object, ga) = (self.device, self.object, self.ga);
        match self.kind {
            ConflictKind::PlainSender => format!(
                "{device} object {object} sends on {ga}, but {device} has no KNX Data Secure and \
                 {others} receive {ga} secured, so they drop every telegram it sends. Either make \
                 {ga} plain (no group key, `secure` off, and the receivers' objects unsecured), or \
                 send on a plain group address into a logic input of the receiver"
            ),
            ConflictKind::PlainReceiver => format!(
                "{device} object {object} listens to {ga}, but {device} has no KNX Data Secure \
                 and {others} send {ga} secured, so it cannot decrypt what it receives. Either \
                 make {ga} plain (no group key, `secure` off, and the senders' objects \
                 unsecured), or have the sender also send on a plain group address that {device} \
                 listens to"
            ),
        }
    }

    /// The validation diagnostic for this conflict (an error).
    pub fn diagnostic(&self) -> Diagnostic {
        Diagnostic::new(
            self.code(),
            Severity::Error,
            format!("links.\"{}\"[{}]", self.device, self.index),
            self.message(),
        )
    }
}

/// One linked object on a group address, as the checks see it.
struct Endpoint {
    device: IndividualAddress,
    object: u16,
    index: usize,
    sends: bool,
    listens: bool,
    /// The device can run Data Secure.
    capable: bool,
    /// The object runs this GA secured: its `secure` flag, or the GA's
    /// (ETS `Security="Auto"` secures an object linked to a secured GA).
    secured: bool,
}

/// Whether `ia` is a device in the model that can run KNX Data Secure.
fn secure_capable(model: &Model, ia: IndividualAddress) -> bool {
    model
        .devices
        .get(&ia)
        .and_then(|d| d.device.security.as_ref())
        .is_some_and(|s| s.secure_capable)
}

/// Every link in `model` that puts a device without Data Secure on a group
/// address the other side runs secured (E035 and E036), in a stable order.
/// A link of a device the model has no file for is skipped (that is E013).
pub fn secure_link_conflicts(model: &Model) -> Vec<SecureLinkConflict> {
    let mut by_ga: BTreeMap<GroupAddress, Vec<Endpoint>> = BTreeMap::new();
    for (ia, links) in &model.links.links {
        let Some(loaded) = model.devices.get(ia) else {
            continue;
        };
        let capable = secure_capable(model, *ia);
        for (index, link) in links.iter().enumerate() {
            let object_secure = loaded
                .device
                .com_objects
                .get(&link.object)
                .is_some_and(|co| co.secure);
            let mut gas: Vec<GroupAddress> = link.send.into_iter().collect();
            gas.extend(link.listen.iter().copied());
            gas.sort();
            gas.dedup();
            for ga in gas {
                let group_secure = model.groups.groups.get(&ga).is_some_and(|g| g.secure);
                by_ga.entry(ga).or_default().push(Endpoint {
                    device: *ia,
                    object: link.object,
                    index,
                    sends: link.send == Some(ga),
                    listens: link.listen.contains(&ga),
                    capable,
                    secured: capable && (object_secure || group_secure),
                });
            }
        }
    }

    let mut out = Vec::new();
    for (ga, endpoints) in &by_ga {
        for plain in endpoints.iter().filter(|e| !e.capable) {
            let others = |want: fn(&Endpoint) -> bool| -> Vec<(IndividualAddress, u16)> {
                endpoints
                    .iter()
                    .filter(|e| e.device != plain.device && e.secured && want(e))
                    .map(|e| (e.device, e.object))
                    .collect()
            };
            if plain.sends {
                let receivers = others(|e| e.listens);
                if !receivers.is_empty() {
                    out.push(SecureLinkConflict {
                        kind: ConflictKind::PlainSender,
                        device: plain.device,
                        object: plain.object,
                        index: plain.index,
                        ga: *ga,
                        secured: receivers,
                    });
                }
            }
            if plain.listens {
                let senders = others(|e| e.sends);
                if !senders.is_empty() {
                    out.push(SecureLinkConflict {
                        kind: ConflictKind::PlainReceiver,
                        device: plain.device,
                        object: plain.object,
                        index: plain.index,
                        ga: *ga,
                        secured: senders,
                    });
                }
            }
        }
    }
    out
}

/// The validation pass: one E035 or E036 error per conflict.
pub(crate) fn check_secure_links(model: &Model, diags: &mut Vec<Diagnostic>) {
    diags.extend(secure_link_conflicts(model).iter().map(|c| c.diagnostic()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoadedDevice;
    use crate::schema::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn com_object(secure: bool) -> Result<ComObject, Box<dyn std::error::Error>> {
        Ok(ComObject {
            dpt: Some("1.001".parse()?),
            flags: "CWTU".parse()?,
            secure,
            ..ComObject::default()
        })
    }

    fn device(
        addr: &str,
        capable: bool,
        object: u16,
        secure_object: bool,
    ) -> Result<LoadedDevice, Box<dyn std::error::Error>> {
        let address: IndividualAddress = addr.parse()?;
        Ok(LoadedDevice {
            device: Device {
                address,
                name: format!("dev {addr}"),
                description: None,
                location: None,
                replaced: None,
                product: None,
                channels: BTreeMap::new(),
                parameters: BTreeMap::new(),
                module_bases: BTreeMap::new(),
                com_objects: [(object, com_object(secure_object)?)].into_iter().collect(),
                security: capable.then(|| DeviceSecurity {
                    secure_capable: true,
                    ..DeviceSecurity::default()
                }),
                application_override: None,
                knxnet: None,
                lock: Default::default(),
            },
            file_stem: addr.to_string(),
        })
    }

    /// A model with GA 1/0/4 (`secure` as given), the secured actuator 1.1.5
    /// listening on object 3, and the plain weather station 1.1.202 whose
    /// object 18 is linked with `send` or `listen`.
    fn model(ga_secure: bool, plain_sends: bool) -> Result<Model, Box<dyn std::error::Error>> {
        let ga: GroupAddress = "1/0/4".parse()?;
        let mut groups = BTreeMap::new();
        groups.insert(
            ga,
            Group {
                name: "Licht Küche aussen Schalten".to_string(),
                dpt: Some("1.001".parse()?),
                description: None,
                protected: false,
                secure: ga_secure,
            },
        );
        let mut links = BTreeMap::new();
        links.insert(
            "1.1.5".parse()?,
            vec![Link {
                object: 3,
                name: None,
                send: (!plain_sends).then_some(ga),
                listen: if plain_sends { vec![ga] } else { Vec::new() },
            }],
        );
        links.insert(
            "1.1.202".parse()?,
            vec![Link {
                object: 18,
                name: None,
                send: plain_sends.then_some(ga),
                listen: if plain_sends { Vec::new() } else { vec![ga] },
            }],
        );
        let mut devices = BTreeMap::new();
        devices.insert("1.1.5".parse()?, device("1.1.5", true, 3, ga_secure)?);
        devices.insert("1.1.202".parse()?, device("1.1.202", false, 18, false)?);
        Ok(Model {
            config: BussardConfig::default(),
            groups: Groups {
                project: None,
                imported_from: None,
                ranges: BTreeMap::new(),
                groups,
            },
            links: Links { links },
            devices,
        })
    }

    #[test]
    fn test_secure_link_conflicts_plain_sender_on_secured_ga_is_e035() -> TestResult {
        let found = secure_link_conflicts(&model(true, true)?);
        assert_eq!(found.len(), 1, "{found:?}");
        let conflict = &found[0];
        assert_eq!(conflict.code(), "E035");
        assert_eq!(conflict.device, "1.1.202".parse()?);
        let diag = conflict.diagnostic();
        assert_eq!(diag.severity, Severity::Error);
        assert_eq!(diag.location, "links.\"1.1.202\"[0]");
        for needle in [
            "1.1.202 object 18",
            "1/0/4",
            "1.1.5 object 3",
            "plain",
            "logic input",
        ] {
            assert!(diag.message.contains(needle), "{needle}: {}", diag.message);
        }
        Ok(())
    }

    #[test]
    fn test_secure_link_conflicts_plain_ga_is_clean() -> TestResult {
        assert!(secure_link_conflicts(&model(false, true)?).is_empty());
        assert!(secure_link_conflicts(&model(false, false)?).is_empty());
        Ok(())
    }

    #[test]
    fn test_secure_link_conflicts_secure_object_on_plain_ga_counts() -> TestResult {
        // The receiver's object is marked secure although the GA flag is off.
        let mut m = model(false, true)?;
        let actuator: IndividualAddress = "1.1.5".parse()?;
        if let Some(d) = m.devices.get_mut(&actuator)
            && let Some(co) = d.device.com_objects.get_mut(&3)
        {
            co.secure = true;
        }
        let codes: Vec<&str> = secure_link_conflicts(&m).iter().map(|c| c.code()).collect();
        assert_eq!(codes, vec!["E035"]);
        Ok(())
    }

    #[test]
    fn test_secure_link_conflicts_plain_receiver_of_secure_sender_is_e036() -> TestResult {
        let found = secure_link_conflicts(&model(true, false)?);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].code(), "E036");
        let message = found[0].message();
        assert!(message.contains("cannot decrypt"), "{message}");
        assert!(message.contains("1.1.5 object 3"), "{message}");
        Ok(())
    }

    #[test]
    fn test_secure_link_conflicts_both_secure_is_clean() -> TestResult {
        let mut m = model(true, true)?;
        let station: IndividualAddress = "1.1.202".parse()?;
        if let Some(d) = m.devices.get_mut(&station) {
            d.device.security = Some(DeviceSecurity {
                secure_capable: true,
                ..DeviceSecurity::default()
            });
        }
        assert!(secure_link_conflicts(&m).is_empty());
        Ok(())
    }

    #[test]
    fn test_validate_reports_e035() -> TestResult {
        let diags = crate::validate::validate(&model(true, true)?);
        assert!(diags.iter().any(|d| d.code == "E035"), "{diags:?}");
        assert!(crate::validate::has_errors(&diags));
        Ok(())
    }
}
