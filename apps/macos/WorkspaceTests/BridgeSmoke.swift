import Foundation

@main
struct BridgeSmoke {
    static func main() throws {
        let app = WorkspaceApplication()
        let before = try app.snapshot()
        precondition(before.counter == 0 && before.revision == 0)
        let after = try app.increment()
        precondition(after.counter == 1 && after.revision == 1)
        print("Swift → UniFFI → Rust smoke test passed")
    }
}
