import SwiftUI

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore

    var body: some View {
        VStack(spacing: 16) {
            Text("Counter: \(store.counter)")
                .font(.title2)
            Button("Increment") { store.increment() }
                .buttonStyle(.glassProminent)
                .controlSize(.large)
                .keyboardShortcut("+", modifiers: .command)
            if let error = store.errorMessage {
                Text(error).foregroundStyle(.red)
            }
        }
        .frame(minWidth: 320, minHeight: 180)
        .padding()
    }
}
