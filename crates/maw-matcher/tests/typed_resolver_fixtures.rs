use maw_matcher::{normalized_match_names, resolve_typed_target, ResolveCandidateKind, ResolveMatch, ResolveMatchRank, ResolveTypedCandidate, ResolveTypedResult};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Fixture { name: String, target: String, candidates: Vec<Candidate>, expected: Value }
#[derive(Deserialize)]
struct Candidate { kind: String, name: String, #[serde(default)] aliases: Vec<String> }

#[test]
fn typed_resolver_fixture_table() {
    assert_eq!(normalized_match_names("81-Kru32-Oracle"), ["81-kru32", "81-kru32-oracle", "kru32", "kru32-oracle"]);
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("fixtures/typed-resolver.fixtures.json")).expect("valid typed resolver fixtures");
    assert_eq!(fixtures.len(), 7, "#318a fixture count changed");
    for fixture in fixtures {
        let candidates = fixture.candidates.into_iter().map(|c| ResolveTypedCandidate { kind: parse_kind(&c.kind), name: c.name, aliases: c.aliases }).collect::<Vec<_>>();
        assert_eq!(shape(resolve_typed_target(&fixture.target, &candidates)), fixture.expected, "{}", fixture.name);
    }
}

fn shape(result: ResolveTypedResult) -> Value { match result {
    ResolveTypedResult::None => json!({"kind":"none"}),
    ResolveTypedResult::Match { matched } => json!({"kind":"match","match": match_shape(&matched)}),
    ResolveTypedResult::Ambiguous { candidates } => json!({"kind":"ambiguous","candidates": candidates.iter().map(match_shape).collect::<Vec<_>>() }),
} }
fn match_shape(m: &ResolveMatch) -> Value { json!({"kind": format_kind(m.candidate.kind), "name": &m.candidate.name, "rank": format_rank(m.rank)}) }
fn parse_kind(kind: &str) -> ResolveCandidateKind { match kind { "live-session" => ResolveCandidateKind::LiveSession, "sleeping-registry" => ResolveCandidateKind::SleepingRegistry, "fleet-squad" => ResolveCandidateKind::FleetSquad, "oracle" => ResolveCandidateKind::Oracle, "repo" => ResolveCandidateKind::Repo, "window" => ResolveCandidateKind::Window, "peer" => ResolveCandidateKind::Peer, _ => panic!("unknown kind {kind}"), } }
fn format_kind(kind: ResolveCandidateKind) -> &'static str { match kind { ResolveCandidateKind::LiveSession => "live-session", ResolveCandidateKind::SleepingRegistry => "sleeping-registry", ResolveCandidateKind::FleetSquad => "fleet-squad", ResolveCandidateKind::Oracle => "oracle", ResolveCandidateKind::Repo => "repo", ResolveCandidateKind::Window => "window", ResolveCandidateKind::Peer => "peer", } }
fn format_rank(rank: ResolveMatchRank) -> &'static str { match rank { ResolveMatchRank::Exact => "exact", ResolveMatchRank::Live => "live", ResolveMatchRank::Registry => "registry", ResolveMatchRank::HashSlotOwner => "hash-slot-owner", ResolveMatchRank::Fuzzy => "fuzzy", } }
