import Foundation
import Testing
@testable import MawMenubar

@Test func decodesCurrentServeShapes() throws {
    let sessions = try JSONDecoder().decode([FleetSession].self, from: Data(#"[{"name":"33-maw-rs","windows":[{"index":1,"name":"lead","active":true,"cwd":"/repo","panes":[{"id":"%1","command":"claude","target":"33-maw-rs:lead.0","title":"agent","pid":42,"cwd":"/repo","lastActivity":99}]}]}]"#.utf8))
    let agents = try JSONDecoder().decode(AgentsResponse.self, from: Data(#"{"agents":[{"id":"%1","target":"33-maw-rs:lead.0","title":"agent","command":"claude","cwd":"/repo","pid":42,"last_activity":99,"node":"m5"}],"count":1,"node":"m5"}"#.utf8))
    let feed = try JSONDecoder().decode(FeedResponse.self, from: Data(#"{"events":[{"kind":"message","state":"done","oracle":"odin","lastLine":"done #480"}],"total":1,"active_oracles":["odin"]}"#.utf8))
    #expect(sessions[0].windows[0].panes[0].target == "33-maw-rs:lead.0")
    #expect(agents.count == 1 && agents.agents[0].node == "m5" && agents.agents[0].lastActivity == 99)
    #expect(feed.activeOracles == ["odin"] && feed.events[0].summary == "done #480")
}

@Test func decodesPlannedScheduleStatusAndWarning() throws {
    let status = try JSONDecoder().decode(ScheduleStatus.self, from: Data(#"{"activeReservations":1,"recentFailures":1,"staleOutcomes":0,"latest":{"runId":"odin-daily-1","status":"failed","updatedAt":99}}"#.utf8))
    #expect(status.activeReservations == 1 && status.latest?.status == "failed")
}

@Test func actionsKeepHostileTargetInOneArgument() {
    let target = #"pane'; touch /tmp/should-not-exist; echo '"#
    #expect(MawAction.peek(target).arguments == ["peek", target])
    #expect(MawAction.wake(target).arguments == ["wake", target])
}
