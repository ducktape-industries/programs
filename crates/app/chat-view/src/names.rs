//! What the network calls a principal, as the composer offers and inserts it.
//! The roster itself is [`chat::view::Names`], shared with every view that
//! names chat principals.
use std::collections::BTreeMap;

use chat::view::Names;
use chat::{Category, Kind, Principal, Standing};
use identity::{Ownership, Party};

/// Autocomplete candidates: every account ([`Names::people`]), then the
/// room's other members, labelled without the `@`.
pub fn mention_choices(names: &Names, members: &[Principal]) -> Vec<MentionChoice> {
    let accounts = names.people().map(Principal::Account);
    let members = members.iter().filter(|principal| names.pickable(principal));
    let mut choices: Vec<MentionChoice> = Vec::new();
    for principal in accounts.chain(members.cloned()) {
        if !choices.iter().any(|choice| choice.principal == principal) {
            choices.push(MentionChoice {
                label: names.mention(&principal)[1..].to_string(),
                offered: offered(names, &principal),
                principal,
            });
        }
    }
    choices.sort_by_key(|choice| choice.label.to_lowercase());
    choices
}

fn offered(names: &Names, principal: &Principal) -> bool {
    matches!(
        names.kind(principal),
        Some(
            Kind::User
                | Kind::Program {
                    category: Category::Agent,
                    standing: Standing::Active,
                    ..
                }
        )
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionChoice {
    pub label: String,
    pub principal: Principal,
    pub offered: bool,
}

/// The canonical token the composer inserts: `<@account>`.
pub fn mention_token(principal: &Principal) -> String {
    match principal {
        Principal::Account(account) => format!("<@{account}>"),
        Principal::Root => String::new(),
    }
}

/// The account a [`mention_token`] names.
pub fn mentioned(token: &str) -> Option<u64> {
    token.strip_prefix("<@")?.strip_suffix('>')?.parse().ok()
}

pub fn unanswered(
    names: &Names,
    ownerships: &BTreeMap<u64, Ownership>,
    agent: &Principal,
    reader: u64,
) -> Option<&'static str> {
    let Some(Kind::Program {
        manager,
        category: Category::Agent,
        standing: Standing::Active,
        ..
    }) = names.kind(agent)
    else {
        return None;
    };
    if *manager == reader {
        return None;
    }
    match ownerships.get(&agent.account()?)? {
        Ownership::Owner => Some("its manager only"),
        Ownership::Listed(listed) if !listed.contains(&Party::Account(reader)) => {
            Some("listed people only")
        }
        Ownership::Listed(_) | Ownership::Anyone => None,
    }
}

/// Whom an agent answers, in words: its manager, and whom its `Ownership`
/// adds.
pub fn answers(names: &Names, ownership: &Ownership) -> String {
    match ownership {
        Ownership::Owner => "Answers its manager only".into(),
        Ownership::Listed(listed) => {
            let listed: Vec<String> = listed
                .iter()
                .map(|party| match party {
                    Party::Account(number) => names.member(&Principal::Account(*number)),
                    Party::Program(program) => format!("the {program} program"),
                })
                .collect();
            format!("Answers its manager and {}", listed.join(", "))
        }
        Ownership::Anyone => "Answers anyone".into(),
    }
}

/// The other account of a dm room `mine` is in.
pub fn dm_peer_of(mine: u64, channel_id: &str) -> Option<u64> {
    let (a, b) = chat::dm_peers(channel_id)?;
    (mine == a).then_some(b).or((mine == b).then_some(a))
}

#[cfg(not(feature = "notifier"))]
impl crate::Chat {
    pub(crate) fn dm_peer(&self) -> Option<(String, bool)> {
        let room = self.room.as_ref()?;
        let names = self.names.ready()?;
        let peer = Principal::Account(dm_peer_of(self.my_account()?, &room.id)?);
        Some((names.member(&peer), crate::message::agent(names, &peer)))
    }

    pub(crate) fn room_name(&self, id: &str) -> String {
        let peer = self
            .my_account()
            .and_then(|me| dm_peer_of(me, id))
            .zip(self.names.ready())
            .map(|(peer, names)| names.member(&Principal::Account(peer)));
        let name = |name: &str| match chat::dm_peers(id) {
            Some(_) => name.to_owned(),
            None => format!("#{name}"),
        };
        peer.unwrap_or_else(|| match self.info(id) {
            Some(info) => name(&info.channel.name),
            None => name(&view::design::short_hex(id)),
        })
    }

    pub(crate) fn intro(&self) -> Option<String> {
        let room = self.room.as_ref()?;
        Some(match self.dm_peer() {
            Some((peer, _)) => {
                format!("This is the very beginning of your conversation with {peer}.")
            }
            None => {
                let name = self
                    .info(&room.id)
                    .map_or(room.id.as_str(), |info| info.channel.name.as_str());
                format!(
                    "This is the very beginning of #{name}. Say hello, or pin what the room is for."
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use chat::{Category, Kind, Principal, Profile, Standing};

    use super::*;

    fn profile(number: u64, name: &str, kind: Kind) -> Profile {
        Profile {
            number,
            name: name.into(),
            kind,
        }
    }

    fn agent(manager: u64, standing: Standing) -> Kind {
        Kind::Program {
            script: None,
            manager,
            category: Category::Agent,
            standing,
        }
    }

    #[test]
    fn the_pickers_offer_every_account() {
        let names = Names::from_roster([
            profile(1, "ada", Kind::User),
            profile(2, "scout", agent(1, Standing::Active)),
            profile(3, "forge", Kind::System("forge".into())),
            profile(4, "idle", agent(1, Standing::Suspended)),
            profile(5, "gone", agent(1, Standing::Revoked)),
        ]);
        assert_eq!(names.people().collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
        let members = [3, 4, 9].map(Principal::Account);
        let labels: Vec<_> = mention_choices(&names, &members)
            .into_iter()
            .map(|choice| choice.label)
            .collect();
        assert_eq!(
            labels,
            ["account-9", "ada", "forge", "gone", "idle", "scout"]
        );
        let account = Principal::Account;
        assert_eq!(names.member(&account(4)), "idle (suspended)");
        assert_eq!(names.member(&account(5)), "gone (revoked)");
        assert_eq!(names.badge(&account(1)), None);
        assert_eq!(
            names.badge(&account(2)).as_deref(),
            Some("Agent · managed by ada")
        );
        assert_eq!(names.badge(&account(3)).as_deref(), Some("Program · forge"));
        assert_eq!(names.program(&account(3)), Some("forge"));
        assert!(crate::message::agent(&names, &account(2)));
        assert!(!crate::message::agent(&names, &account(3)));
    }

    #[test]
    fn the_mention_menu_offers_people_and_active_agents_only() {
        let names = Names::from_roster([
            profile(1, "ada", Kind::User),
            profile(2, "scout", agent(1, Standing::Active)),
            profile(3, "forge", Kind::System("forge".into())),
            profile(4, "idle", agent(1, Standing::Suspended)),
            profile(5, "gone", agent(1, Standing::Revoked)),
        ]);
        let members = [3, 4, 9].map(Principal::Account);
        let choices = mention_choices(&names, &members);
        let offered: Vec<_> = choices
            .iter()
            .map(|choice| (choice.label.as_str(), choice.offered))
            .collect();
        assert_eq!(
            offered,
            [
                ("account-9", false),
                ("ada", true),
                ("forge", false),
                ("gone", false),
                ("idle", false),
                ("scout", true),
            ]
        );
    }

    #[test]
    fn who_an_agent_leaves_unanswered() {
        let names = Names::from_roster([
            profile(1, "ada", Kind::User),
            profile(2, "scout", agent(1, Standing::Active)),
            profile(3, "forge", Kind::System("forge".into())),
            profile(4, "bo", Kind::User),
            profile(5, "idle", agent(1, Standing::Suspended)),
        ]);
        let scout = Principal::Account(2);
        let heard = |ownership: Ownership, reader| {
            let ownerships = BTreeMap::from([(2, ownership)]);
            unanswered(&names, &ownerships, &scout, reader)
        };
        assert_eq!(heard(Ownership::Owner, 1), None, "its manager");
        assert_eq!(heard(Ownership::Owner, 4), Some("its manager only"));
        let bo = Ownership::Listed(vec![Party::Account(4), Party::Program("forge".into())]);
        assert_eq!(heard(bo.clone(), 4), None);
        assert_eq!(heard(bo, 9), Some("listed people only"));
        assert_eq!(heard(Ownership::Anyone, 9), None);
        let none = BTreeMap::new();
        assert_eq!(unanswered(&names, &none, &scout, 4), None, "unread");
        for other in [1, 3, 5] {
            let ownerships = BTreeMap::from([(other, Ownership::Owner)]);
            assert_eq!(
                unanswered(&names, &ownerships, &Principal::Account(other), 4),
                None
            );
        }
        assert_eq!(
            answers(
                &names,
                &Ownership::Listed(vec![
                    Party::Account(4),
                    Party::Account(9),
                    Party::Program("forge".into())
                ])
            ),
            "Answers its manager and bo, account 9, the forge program"
        );
    }
}
