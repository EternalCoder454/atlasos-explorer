#![no_main]
//! File names, as they are shown, checked, split and renamed in a batch.
use atlas_explorer_core::batch::{self, CaseMode, Edge, Item, Op};
use atlas_explorer_core::{display, names, preview};
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Input<'a> {
    names: Vec<(&'a str, bool)>,
    find: &'a str,
    with: &'a str,
    op: u8,
    flag: bool,
    start: u64,
    step: u64,
    padding: u8,
    raw: &'a [u8],
}

fuzz_target!(|i: Input| {
    // display
    let shown = display::display_name(i.raw);
    assert!(shown.chars().count() <= display::MAX_DISPLAY_CHARS + 1);
    assert!(!shown.chars().any(|c| c.is_control()), "{shown:?}");
    assert!(!shown.chars().any(atlas_explorer_core::launch::is_hidden_char));
    let _ = preview::looks_binary(i.raw);
    let _ = preview::sanitize_text(i.raw, false);
    // a typed name
    if let Ok(_w) = names::validate(i.find) {
        assert!(!i.find.is_empty() && !i.find.contains(['/', '\0']) && i.find != "." && i.find != "..");
        assert!(i.find.len() <= names::MAX_NAME_BYTES);
    }
    let _ = names::stem_len(i.find, i.flag);
    let kept = names::keep_both_name(i.find, |n| n.len() % 3 == 0);
    assert!(kept.len() <= names::MAX_NAME_BYTES + 8);
    // batch rename
    let items: Vec<Item> = i.names.iter().take(64).map(|(n, d)| Item { name: n, is_dir: *d }).collect();
    let sep = i.with;
    let op = match i.op % 4 {
        0 => Op::Replace { find: i.find, with: i.with, match_case: i.flag, regex: i.step % 2 == 0 },
        1 => Op::Number {
            start: i.start,
            step: i.step,
            padding: usize::from(i.padding),
            at: if i.flag { Edge::End } else { Edge::Start },
            separator: sep,
        },
        2 => Op::Case(match i.padding % 4 {
            0 => CaseMode::Lower,
            1 => CaseMode::Upper,
            2 => CaseMode::Title,
            _ => CaseMode::Sentence,
        }),
        _ => Op::AddText { text: i.with, at: if i.flag { Edge::End } else { Edge::Start } },
    };
    let plan = batch::plan(&items, &op, |n| n.len() % 5 == 0);
    for row in &plan.rows {
        // (an item that does not change is left alone, whatever it is called)
        if !row.check.blocks() && row.check != batch::Check::Unchanged {
            let n = row.new.as_str();
            assert!(!n.is_empty() && n != "." && n != "..", "{n:?} {:?} for {:?}", row.check, items.iter().map(|i| i.name).collect::<Vec<_>>());
            assert!(!n.contains(['/', '\0']), "{n:?}");
            assert!(n.len() <= names::MAX_NAME_BYTES, "{}", n.len());
        }
    }
});
