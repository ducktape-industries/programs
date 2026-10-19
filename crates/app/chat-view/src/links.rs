//! Chat's `duck://` links: a room or a message to open, and the route
//! the app hands back when one is opened.

/// `duck://<chain>/chat/<channel>[/<seq>]`: chat's own tail, the channel
/// percent-encoded by `ducklink` like any segment (`forge:web:3` →
/// `forge%3Aweb%3A3`); none without a chain.
pub fn channel_link(chain: &str, channel: &str, seq: Option<u64>) -> Option<String> {
    let seq = seq.map(|seq| seq.to_string());
    let mut tail = vec![channel];
    tail.extend(seq.as_deref());
    ducklink::mint(chain, ::chat::PROGRAM, &tail)
}

// `duck://<chain>/files/<segment>/…`: where Files shows what is at `path`
// (`/chat/general/m3/tape.png` is `duck://<chain>/files/chat/general/m3/tape.png`);
// none without a chain.
pub(crate) fn file_link(chain: &str, path: &str) -> Option<String> {
    let tail: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    ducklink::mint(chain, ::files::PROGRAM, &tail)
}

/// Where a program's room (`forge:web:3`) is shown by its program:
/// `duck://<chain>/forge/web/3`, the room id's own path
/// ([`chat::namespace::program`]). None for a room people opened, or no chain yet.
pub fn program_link(chain: &str, channel: &str) -> Option<String> {
    let program = ::chat::namespace::program(channel)?;
    let path: Vec<&str> = channel[program.len() + 1..].split(':').collect();
    ducklink::mint(chain, program, &path)
}

/// `duck://<chain>/<calls>/<agent>/chat/<channel>`: where the program an
/// agent's script calls shows its work in a room, by the place it keeps it
/// under (`chat/<channel>`), the channel percent-encoded like any segment.
/// None without a chain.
pub fn work_record(chain: &str, calls: &str, agent: u64, channel: &str) -> Option<String> {
    let agent = agent.to_string();
    ducklink::mint(chain, calls, &[&agent, ::chat::PROGRAM, channel])
}

/// `duck://explorer/block/<height>`: the host's short link to a view on
/// this connection, Explorer at that block.
pub fn block_link(height: u64) -> String {
    format!("duck://explorer/block/{height}")
}

/// The room and message a route handed to this view names:
/// `<channel>[/<seq>]`, delivered decoded, the seq 0 when there is none.
pub fn route_target(route: &str) -> Option<(String, u64)> {
    let mut parts = route.splitn(2, '/');
    let channel = parts.next()?.to_owned();
    let seq = parts.next().and_then(|seq| seq.parse().ok()).unwrap_or(0);
    (!channel.is_empty()).then_some((channel, seq))
}

/// A pressed mention (an account number) becomes `duck://<chain>/identity/<n>`,
/// the link the app opens; any other link is already one and passes through.
/// None when no link can be minted (no chain yet).
pub fn pressed_link(link: String, chain: &str) -> Option<String> {
    match link.parse::<u64>() {
        Ok(account) => ducklink::mint(chain, identity::PROGRAM, &[&account.to_string()]),
        Err(_) => Some(link),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_keep_their_shapes() {
        assert_eq!(
            channel_link("testnet#0a1b2c3d", "general", Some(42)).as_deref(),
            Some("duck://testnet-0a1b2c3d/chat/general/42")
        );
        assert_eq!(channel_link("", "general", None), None);
        assert_eq!(
            pressed_link("7".into(), "testnet#0a1b2c3d").as_deref(),
            Some("duck://testnet-0a1b2c3d/identity/7")
        );
        assert_eq!(pressed_link("7".into(), ""), None);
        assert_eq!(
            program_link("testnet#0a1b2c3d", "forge:web:3").as_deref(),
            Some("duck://testnet-0a1b2c3d/forge/web/3")
        );
        assert_eq!(program_link("testnet#0a1b2c3d", "general"), None);
        assert_eq!(
            work_record("testnet#0a1b2c3d", "harness", 12, "general").as_deref(),
            Some("duck://testnet-0a1b2c3d/harness/12/chat/general")
        );
        let link = work_record("testnet#0a1b2c3d", "harness", 12, "forge:web:3").unwrap();
        assert_eq!(
            link,
            "duck://testnet-0a1b2c3d/harness/12/chat/forge%3Aweb%3A3"
        );
        assert_eq!(
            ducklink::Link::parse(&link).unwrap().tail,
            ["12", "chat", "forge:web:3"]
        );
        assert_eq!(work_record("", "harness", 12, "general"), None);
    }

    /// Any room lands, a forge room's `:` included: the link spells the id
    /// percent-encoded, and the view reads the route the app hands it
    /// (the tail, decoded and joined) back to the same id and message.
    #[test]
    fn a_channel_link_round_trips_through_the_apps_route() {
        let long = format!("forge:{}:{}", "r".repeat(37), u64::MAX);
        for channel in [
            "general",
            "dm-3-5",
            "forge:big-history:3",
            "forge:my.lib:12",
            "a.2E",
            "보고서 #1",
            "50% off?",
            long.as_str(),
        ] {
            let link = channel_link("testnet#0a1b2c3d", channel, Some(42)).unwrap();
            // what the app does with a chain link: the tail, decoded, joined
            let route = ducklink::Link::parse(&link).unwrap().tail.join("/");
            assert_eq!(route_target(&route), Some((channel.to_owned(), 42)));
        }
        assert_eq!(
            channel_link("testnet#0a1b2c3d", "forge:web:3", None).as_deref(),
            Some("duck://testnet-0a1b2c3d/chat/forge%3Aweb%3A3")
        );
        assert_eq!(route_target("design"), Some(("design".into(), 0)));
        assert_eq!(
            route_target("forge:web:3/7"),
            Some(("forge:web:3".into(), 7))
        );
    }
}
