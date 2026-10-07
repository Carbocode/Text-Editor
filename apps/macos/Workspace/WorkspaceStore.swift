import Foundation

@MainActor
final class WorkspaceStore: ObservableObject {
    @Published private(set) var counter: UInt64 = 0
    @Published private(set) var revision: UInt64 = 0
    @Published private(set) var errorMessage: String?

    private let core = WorkspaceApplication()

    init() {
        do {
            apply(try core.snapshot())
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func increment() {
        do {
            apply(try core.increment())
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func apply(_ snapshot: WorkspaceSnapshot) {
        revision = snapshot.revision
        counter = snapshot.counter
    }
}
