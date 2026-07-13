import AppKit
import Foundation

struct Health: Decodable, Sendable { let ok: Bool; let source: String; let server: String; let port: Int }
struct Pane: Decodable, Sendable {
    let id, command, target, title: String
    let pid: Int?; let cwd: String?; let lastActivity: UInt64?
}
struct FleetWindow: Decodable, Sendable {
    let index: Int; let name: String; let active: Bool; let cwd: String?; let panes: [Pane]
}
struct FleetSession: Decodable, Sendable { let name: String; let windows: [FleetWindow] }
struct Agent: Decodable, Sendable {
    let id, target, title, command: String
    let cwd: String?; let pid: Int?; let lastActivity: UInt64?; let node: String?
    enum CodingKeys: String, CodingKey {
        case id, target, title, command, cwd, pid, node
        case lastActivity = "last_activity"
    }
}
struct AgentsResponse: Decodable, Sendable { let agents: [Agent]; let count: Int; let node: String? }
struct FeedEvent: Decodable, Sendable {
    let kind: String?; let state: String?; let oracle: String?; let lastLine: String?; let text: String?
    var summary: String { lastLine ?? text ?? [kind, state].compactMap { $0 }.joined(separator: ": ") }
}
struct FeedResponse: Decodable, Sendable {
    let events: [FeedEvent]; let total: Int; let activeOracles: [String]
    enum CodingKeys: String, CodingKey { case events, total; case activeOracles = "active_oracles" }
}
struct ScheduleRun: Decodable, Sendable { let runId, status: String; let updatedAt: UInt64 }
struct ScheduleStatus: Decodable, Sendable {
    let activeReservations, recentFailures, staleOutcomes: Int; let latest: ScheduleRun?
}
struct FleetSnapshot: Sendable {
    let health: Health; let sessions: [FleetSession]; let agents: AgentsResponse
    let feed: FeedResponse; let schedule: ScheduleStatus?; let receivedAt: Date
    var warning: Bool { schedule.map { $0.recentFailures > 0 || $0.staleOutcomes > 0 } ?? false }
}

enum APIError: Error { case invalidURL, http(Int) }
struct APIClient: Sendable {
    let baseURL: URL; let session: URLSession
    func get<T: Decodable & Sendable>(_ path: String, as: T.Type) async throws -> T {
        guard let url = URL(string: path, relativeTo: baseURL) else { throw APIError.invalidURL }
        let (data, response) = try await session.data(from: url)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else { throw APIError.http(status) }
        return try JSONDecoder().decode(T.self, from: data)
    }
    func poll() async throws -> FleetSnapshot {
        async let health = get("/api/health", as: Health.self)
        async let sessions = get("/api/sessions?local=true", as: [FleetSession].self)
        async let agents = get("/api/agents", as: AgentsResponse.self)
        async let feed = get("/api/feed?limit=20", as: FeedResponse.self)
        let core = try await (health, sessions, agents, feed)
        let schedule = try? await get("/api/schedule/status", as: ScheduleStatus.self)
        return FleetSnapshot(health: core.0, sessions: core.1, agents: core.2,
            feed: core.3, schedule: schedule, receivedAt: Date())
    }
}

enum MawAction: Sendable {
    case peek(String), wake(String)
    var arguments: [String] {
        switch self { case .peek(let target): ["peek", target]; case .wake(let target): ["wake", target] }
    }
}
struct ActionResult: Sendable { let exitCode: Int32; let output: String }
struct MawActionRunner: Sendable {
    let executable: URL
    func run(_ action: MawAction) -> ActionResult {
        let process = Process(); let pipe = Pipe()
        process.executableURL = executable; process.arguments = action.arguments
        process.standardOutput = pipe; process.standardError = pipe
        do { try process.run() } catch { return ActionResult(exitCode: 127, output: String(describing: error)) }
        let data = pipe.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
        return ActionResult(exitCode: process.terminationStatus,
            output: String(decoding: data.prefix(500), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
    }
}

@MainActor final class AppDelegate: NSObject, NSApplicationDelegate {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let client: APIClient; private let runner: MawActionRunner
    private var timer: Timer?; private var snapshot: FleetSnapshot?; private var lastAction = ""; private var isPolling = false
    init(client: APIClient, runner: MawActionRunner) { self.client = client; self.runner = runner }
    func applicationDidFinishLaunching(_ notification: Notification) {
        item.button?.image = NSImage(systemSymbolName: "circle.grid.2x2.fill", accessibilityDescription: "maw")
        item.button?.image?.isTemplate = true; refresh()
        timer = .scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
    }
    @objc private func refresh() {
        guard !isPolling else { return }; isPolling = true
        Task { defer { isPolling = false }
            do { snapshot = try await client.poll(); render() } catch { renderDisconnected(error) }
        }
    }
    private func render() {
        guard let snapshot else { return }; let menu = NSMenu()
        item.button?.title = " \(snapshot.warning ? "!" : String(snapshot.agents.count))"
        menu.addItem(.label("Connected · \(snapshot.health.source):\(snapshot.health.port)"))
        menu.addItem(.label("\(snapshot.sessions.count) sessions · \(snapshot.agents.count) agents"))
        if let schedule = snapshot.schedule {
            menu.addItem(.label("Schedule: \(schedule.activeReservations) active · \(schedule.recentFailures) failed"))
        } else { menu.addItem(.label("Schedule: API unavailable")) }
        for agent in snapshot.agents.agents.prefix(10) { menu.addItem(targetMenu(agent.target)) }
        for event in snapshot.feed.events.suffix(5) where !event.summary.isEmpty { menu.addItem(.label(event.summary)) }
        if !lastAction.isEmpty { menu.addItem(.separator()); menu.addItem(.label(lastAction)) }
        menu.addItem(.separator()); let refresh = NSMenuItem(title: "Refresh now", action: #selector(refresh), keyEquivalent: "r")
        refresh.target = self; menu.addItem(refresh)
        let quit = NSMenuItem(title: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        quit.target = NSApp; menu.addItem(quit)
        item.menu = menu
    }
    private func targetMenu(_ target: String) -> NSMenuItem {
        let parent = NSMenuItem(title: target, action: nil, keyEquivalent: ""); let submenu = NSMenu(title: target)
        for (title, selector) in [("Peek", #selector(peek(_:))), ("Wake", #selector(wake(_:)))] {
            let child = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            child.target = self; child.representedObject = target; submenu.addItem(child)
        }
        parent.submenu = submenu; return parent
    }
    @objc private func peek(_ sender: NSMenuItem) { run(.peek(sender.representedObject as? String ?? "")) }
    @objc private func wake(_ sender: NSMenuItem) { run(.wake(sender.representedObject as? String ?? "")) }
    private func run(_ action: MawAction) {
        let runner = runner
        Task { let result = await Task.detached { runner.run(action) }.value
            lastAction = result.exitCode == 0 ? "Action succeeded" : "Action failed (\(result.exitCode)): \(result.output)"; render() }
    }
    private func renderDisconnected(_ error: Error) {
        item.button?.title = " ×"; let menu = NSMenu(); menu.addItem(.label("Not connected: \(error)"))
        if let snapshot { menu.addItem(.label("Last snapshot: \(Int(Date().timeIntervalSince(snapshot.receivedAt)))s ago")) }
        let refresh = NSMenuItem(title: "Refresh now", action: #selector(refresh), keyEquivalent: "r")
        refresh.target = self; menu.addItem(refresh); item.menu = menu
    }
}

private extension NSMenuItem {
    static func label(_ title: String) -> NSMenuItem { let item = NSMenuItem(title: title, action: nil, keyEquivalent: ""); item.isEnabled = false; return item }
}

@main struct MawMenubar {
    @MainActor static func main() {
        let arguments = ProcessInfo.processInfo.arguments
        let maw = arguments.option("--maw").map(URL.init(fileURLWithPath:)) ?? URL(fileURLWithPath: "/usr/local/bin/maw")
        let api = arguments.option("--api").flatMap(URL.init(string:)) ?? URL(string: "http://127.0.0.1:3456")!
        let config = URLSessionConfiguration.ephemeral; config.timeoutIntervalForRequest = 2
        let app = NSApplication.shared; app.setActivationPolicy(.accessory)
        let delegate = AppDelegate(client: APIClient(baseURL: api, session: URLSession(configuration: config)), runner: MawActionRunner(executable: maw))
        app.delegate = delegate; app.run(); withExtendedLifetime(delegate) {}
    }
}
private extension [String] {
    func option(_ name: String) -> String? { guard let index = firstIndex(of: name), indices.contains(index + 1) else { return nil }; return self[index + 1] }
}
