//! The reaction picker's emoji: a small hand-picked set, grouped the way a
//! reader looks for one, each with the words search matches. No skin tones,
//! no flags, no joined sequences: one glyph the bundled emoji face draws.

/// One tab of the picker: its glyph, its name, and its emoji with the words
/// they answer to.
pub struct Category {
    pub glyph: &'static str,
    pub name: &'static str,
    pub emoji: &'static [(&'static str, &'static str)],
}

/// What the "Frequently used" row shows before the reader has reacted with
/// anything, and fills with after.
pub const USUAL: [&str; 8] = ["👍", "❤️", "😄", "🎉", "👀", "🙏", "🔥", "✅"];

/// How many reactions the "Frequently used" row keeps.
pub const RECENT: usize = 8;

/// The most one tab holds: five rows of eight, the picker's whole grid, so
/// it never scrolls. Every cell is part of each frame the view sends while
/// the picker is open, and chat's frames are already heavy.
pub const PER_TAB: usize = 40;

pub const CATEGORIES: [Category; 8] = [
    Category {
        glyph: "😀",
        name: "Smileys",
        emoji: &[
            ("😀", "grinning smile happy"),
            ("😃", "smiley happy"),
            ("😄", "smile happy"),
            ("😁", "grin beaming"),
            ("😆", "laughing"),
            ("😅", "sweat smile phew"),
            ("😂", "joy tears laugh lol"),
            ("🤣", "rofl rolling laugh"),
            ("😉", "wink"),
            ("😊", "blush"),
            ("😇", "innocent halo"),
            ("🥰", "love hearts"),
            ("😍", "heart eyes love"),
            ("😘", "kiss"),
            ("😋", "yum tasty"),
            ("🤪", "zany crazy"),
            ("🤔", "thinking hmm"),
            ("🤨", "raised eyebrow skeptic"),
            ("😐", "neutral meh"),
            ("🙄", "eye roll"),
            ("😏", "smirk"),
            ("😬", "grimace awkward"),
            ("😌", "relieved"),
            ("😴", "sleeping tired zzz"),
            ("🤯", "mind blown exploding"),
            ("😎", "cool sunglasses"),
            ("🤓", "nerd"),
            ("😕", "confused"),
            ("😮", "open mouth wow surprised"),
            ("😲", "astonished shocked"),
            ("😳", "flushed embarrassed"),
            ("🥺", "pleading puppy eyes"),
            ("😢", "cry sad tear"),
            ("😭", "sob crying"),
            ("😱", "scream fear"),
            ("😡", "angry mad rage"),
            ("💀", "skull dead"),
            ("🤡", "clown"),
            ("👻", "ghost"),
            ("🤖", "robot bot"),
        ],
    },
    Category {
        glyph: "👋",
        name: "People",
        emoji: &[
            ("👍", "thumbs up yes ok +1 like"),
            ("👎", "thumbs down no -1 dislike"),
            ("👋", "wave hello hi bye"),
            ("🤚", "raised back hand"),
            ("✋", "raised hand stop high five"),
            ("👌", "ok perfect"),
            ("✌️", "victory peace"),
            ("🤞", "fingers crossed luck"),
            ("🤟", "love you"),
            ("🤘", "rock on horns"),
            ("👈", "point left"),
            ("👉", "point right"),
            ("👆", "point up"),
            ("👇", "point down"),
            ("☝️", "index up"),
            ("✊", "raised fist"),
            ("👊", "fist bump punch"),
            ("👏", "clap applause"),
            ("🙌", "raised hands hooray celebrate"),
            ("👐", "open hands"),
            ("🤲", "palms up"),
            ("🤝", "handshake deal agree"),
            ("🙏", "pray please thanks"),
            ("💪", "muscle strong flex"),
            ("🧠", "brain smart"),
            ("👀", "eyes look watching"),
            ("🫡", "salute"),
            ("🤷", "shrug dunno"),
            ("🤦", "facepalm"),
            ("🙋", "raising hand me"),
            ("🙇", "bow sorry"),
            ("💅", "nail polish"),
        ],
    },
    Category {
        glyph: "🐶",
        name: "Nature",
        emoji: &[
            ("🐶", "dog puppy"),
            ("🐱", "cat kitten"),
            ("🦊", "fox"),
            ("🐻", "bear"),
            ("🐼", "panda"),
            ("🐨", "koala"),
            ("🐯", "tiger"),
            ("🦁", "lion"),
            ("🐷", "pig"),
            ("🐸", "frog"),
            ("🐵", "monkey"),
            ("🐧", "penguin"),
            ("🐦", "bird"),
            ("🦆", "duck"),
            ("🦉", "owl"),
            ("🐝", "bee"),
            ("🐛", "bug caterpillar"),
            ("🦋", "butterfly"),
            ("🐢", "turtle slow"),
            ("🐍", "snake python"),
            ("🐙", "octopus"),
            ("🐬", "dolphin"),
            ("🐳", "whale"),
            ("🌵", "cactus"),
            ("🌳", "tree"),
            ("🌴", "palm tree"),
            ("🌱", "seedling sprout"),
            ("🍀", "clover luck"),
            ("🍁", "maple leaf"),
            ("🌸", "cherry blossom flower"),
            ("🌻", "sunflower"),
            ("🌹", "rose"),
            ("🌈", "rainbow"),
            ("☀️", "sun sunny"),
            ("🌙", "moon night"),
            ("⭐", "star"),
            ("⚡", "lightning zap fast"),
            ("🔥", "fire lit hot"),
            ("❄️", "snowflake cold"),
            ("🌊", "wave ocean"),
        ],
    },
    Category {
        glyph: "🍕",
        name: "Food",
        emoji: &[
            ("🍎", "apple"),
            ("🍊", "orange tangerine"),
            ("🍋", "lemon"),
            ("🍌", "banana"),
            ("🍉", "watermelon"),
            ("🍇", "grapes"),
            ("🍓", "strawberry"),
            ("🍒", "cherries"),
            ("🍑", "peach"),
            ("🍍", "pineapple"),
            ("🥑", "avocado"),
            ("🍅", "tomato"),
            ("🌶️", "pepper hot spicy"),
            ("🌽", "corn"),
            ("🥕", "carrot"),
            ("🍞", "bread"),
            ("🥐", "croissant"),
            ("🧀", "cheese"),
            ("🥚", "egg"),
            ("🍳", "cooking fried egg"),
            ("🥓", "bacon"),
            ("🍔", "burger hamburger"),
            ("🍟", "fries"),
            ("🍕", "pizza"),
            ("🌮", "taco"),
            ("🍣", "sushi"),
            ("🍜", "ramen noodles"),
            ("🍩", "doughnut donut"),
            ("🍪", "cookie"),
            ("🎂", "birthday cake"),
            ("🍰", "cake shortcake"),
            ("🍫", "chocolate"),
            ("🍿", "popcorn"),
            ("☕", "coffee tea hot"),
            ("🍵", "tea matcha"),
            ("🍺", "beer"),
            ("🍷", "wine"),
            ("🥂", "cheers toast"),
        ],
    },
    Category {
        glyph: "⚽",
        name: "Activity",
        emoji: &[
            ("🎉", "tada party celebrate hooray"),
            ("🎊", "confetti"),
            ("🎈", "balloon"),
            ("🎁", "gift present"),
            ("🏆", "trophy win"),
            ("🥇", "gold medal first"),
            ("🏅", "medal"),
            ("🎯", "target bullseye goal"),
            ("⚽", "soccer football"),
            ("🏀", "basketball"),
            ("🏈", "american football"),
            ("⚾", "baseball"),
            ("🎾", "tennis"),
            ("🏓", "ping pong"),
            ("🎳", "bowling"),
            ("🎮", "video game controller"),
            ("🎲", "dice game"),
            ("🧩", "puzzle piece"),
            ("🎨", "art palette"),
            ("🎭", "theater drama"),
            ("🎬", "clapper movie"),
            ("🎤", "microphone sing"),
            ("🎧", "headphones music"),
            ("🎸", "guitar"),
            ("🎹", "piano keyboard"),
            ("🥁", "drum"),
        ],
    },
    Category {
        glyph: "🚗",
        name: "Travel",
        emoji: &[
            ("🚀", "rocket ship launch"),
            ("✈️", "airplane plane"),
            ("🚗", "car"),
            ("🚕", "taxi"),
            ("🚌", "bus"),
            ("🚓", "police car"),
            ("🚑", "ambulance"),
            ("🚒", "fire engine"),
            ("🚲", "bicycle bike"),
            ("🛴", "scooter"),
            ("🚁", "helicopter"),
            ("🛸", "ufo flying saucer"),
            ("⛵", "sailboat"),
            ("🚢", "ship"),
            ("🚂", "train locomotive"),
            ("🗺️", "map"),
            ("🧭", "compass"),
            ("🏔️", "mountain"),
            ("🏕️", "camping"),
            ("🏖️", "beach"),
            ("🏝️", "island"),
            ("🏠", "house home"),
            ("🏢", "office building"),
            ("🏭", "factory"),
            ("🏰", "castle"),
            ("🌍", "earth globe world"),
            ("🌋", "volcano"),
            ("🚧", "construction wip"),
            ("⚓", "anchor"),
            ("🏁", "checkered flag finish done"),
        ],
    },
    Category {
        glyph: "💡",
        name: "Objects",
        emoji: &[
            ("💡", "bulb idea"),
            ("📱", "phone mobile"),
            ("💻", "laptop computer"),
            ("⌨️", "keyboard"),
            ("🖥️", "desktop computer"),
            ("💾", "floppy save"),
            ("📷", "camera"),
            ("🎥", "movie camera"),
            ("📺", "tv television"),
            ("⏰", "alarm clock"),
            ("⌛", "hourglass time"),
            ("🔋", "battery"),
            ("🔌", "plug"),
            ("🔧", "wrench fix"),
            ("🔨", "hammer"),
            ("🛠️", "tools build"),
            ("⚙️", "gear settings"),
            ("🧰", "toolbox"),
            ("🧲", "magnet"),
            ("🧪", "test tube experiment"),
            ("🔬", "microscope"),
            ("🔭", "telescope"),
            ("📡", "satellite antenna"),
            ("💰", "money bag"),
            ("💳", "credit card"),
            ("💎", "gem diamond"),
            ("🔑", "key"),
            ("🔒", "lock locked"),
            ("🔓", "unlock unlocked"),
            ("📌", "pin pushpin"),
            ("📎", "paperclip"),
            ("✂️", "scissors cut"),
            ("📝", "memo note write"),
            ("✏️", "pencil edit"),
            ("📚", "books docs"),
            ("📦", "package box ship"),
            ("📅", "calendar date"),
            ("📈", "chart up increase"),
            ("📉", "chart down decrease"),
            ("🗑️", "wastebasket trash"),
        ],
    },
    Category {
        glyph: "✅",
        name: "Symbols",
        emoji: &[
            ("✅", "check done yes"),
            ("❌", "cross no wrong"),
            ("➕", "plus add"),
            ("➖", "minus"),
            ("❓", "question"),
            ("❗", "exclamation"),
            ("‼️", "double exclamation"),
            ("💯", "hundred perfect"),
            ("✨", "sparkles shiny new"),
            ("💥", "boom collision"),
            ("💫", "dizzy"),
            ("💤", "zzz sleep"),
            ("💬", "speech comment"),
            ("💭", "thought"),
            ("❤️", "heart love red"),
            ("🧡", "orange heart"),
            ("💛", "yellow heart"),
            ("💚", "green heart"),
            ("💙", "blue heart"),
            ("💜", "purple heart"),
            ("🖤", "black heart"),
            ("🤍", "white heart"),
            ("💔", "broken heart"),
            ("⭕", "circle"),
            ("🚫", "prohibited no"),
            ("⛔", "no entry stop"),
            ("⚠️", "warning caution"),
            ("♻️", "recycle"),
            ("🔄", "arrows refresh"),
            ("🔁", "repeat"),
            ("▶️", "play"),
            ("⏸️", "pause"),
            ("⏩", "fast forward"),
            ("🔔", "bell notification"),
            ("🔕", "muted bell"),
            ("🆗", "ok button"),
            ("🆕", "new"),
            ("🆒", "cool"),
            ("🔝", "top"),
        ],
    },
];

/// The emoji whose words match `query`, each once, in the order the tabs
/// list them: a word that starts with it, or the glyph itself.
pub fn search(query: &str) -> Vec<&'static str> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<&'static str> = Vec::new();
    for (emoji, words) in CATEGORIES.iter().flat_map(|category| category.emoji) {
        let hit = *emoji == query
            || words
                .split(' ')
                .any(|word| word.starts_with(query.as_str()));
        if hit && !found.contains(emoji) {
            found.push(emoji);
        }
    }
    found
}

/// The "Frequently used" row: the reader's own, newest first, filled from
/// [`USUAL`].
pub fn frequent(recent: &[String]) -> Vec<String> {
    let mut row: Vec<String> = recent.iter().take(RECENT).cloned().collect();
    for usual in USUAL {
        if row.len() == RECENT {
            break;
        }
        if !row.iter().any(|emoji| emoji == usual) {
            row.push(usual.into());
        }
    }
    row
}

/// `emoji` to the front of the reader's recent reactions.
pub fn remember(recent: &mut Vec<String>, emoji: &str) {
    recent.retain(|seen| seen != emoji);
    recent.insert(0, emoji.into());
    recent.truncate(RECENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tab_lists_an_emoji_twice_or_overflows_the_grid() {
        for category in &CATEGORIES {
            assert!(
                category.emoji.len() <= PER_TAB,
                "{} holds {}",
                category.name,
                category.emoji.len()
            );
            let mut seen = std::collections::HashSet::new();
            for (emoji, words) in category.emoji {
                assert!(seen.insert(emoji), "{} twice in {}", emoji, category.name);
                assert!(!words.is_empty(), "{emoji} has no words to find it by");
            }
        }
    }

    #[test]
    fn search_matches_word_starts_once_each() {
        assert_eq!(search("thumb"), vec!["👍", "👎"]);
        assert_eq!(search("  FIRE "), vec!["🔥", "🚒"], "fire, fire engine");
        assert!(search("heart").contains(&"❤️"));
        assert_eq!(search("🦆"), vec!["🦆"]);
        assert!(search("zzzzqx").is_empty());
        assert!(search(" ").is_empty());
        assert_eq!(search("zap"), vec!["⚡"]);
    }

    #[test]
    fn frequent_leads_with_the_readers_own_and_fills_from_the_usual() {
        assert_eq!(frequent(&[]), USUAL.map(String::from).to_vec());
        let row = frequent(&["🦆".into(), "🔥".into()]);
        assert_eq!(row.len(), RECENT);
        assert_eq!(&row[..3], ["🦆", "🔥", "👍"]);
        assert_eq!(row.iter().filter(|emoji| *emoji == "🔥").count(), 1);
    }

    #[test]
    fn remembering_moves_to_the_front_and_keeps_eight() {
        let mut recent: Vec<String> = USUAL.map(String::from).to_vec();
        remember(&mut recent, "🦆");
        assert_eq!(recent[0], "🦆");
        assert_eq!(recent.len(), RECENT);
        remember(&mut recent, "👍");
        assert_eq!(&recent[..2], ["👍", "🦆"]);
        assert_eq!(recent.len(), RECENT);
    }
}
