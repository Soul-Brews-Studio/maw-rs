// Discord is already a native top-level command registered in part01 on alpha.
// Gap #10 lives in maw-discord itself; this numbered fragment is intentionally
// empty to reserve part201 for #367 without duplicating dispatcher ownership.
const DISPATCH_201: &[DispatcherEntry] = &[];

#[cfg(test)]
mod discord_tests201 {
    use super::*;

    #[test]
    fn discord_part201_reserves_gap_without_duplicate_dispatch() {
        assert!(DISPATCH_201.is_empty());
        assert_eq!(dispatcher_status("discord"), DispatchKind::Native);
    }
}
