import SwiftUI

struct SidebarTabBar: View {
    @Binding var selection: Int

    private let tabs = [
        ("Files", "folder"),
        ("Search", "magnifyingglass"),
        ("Git", "arrow.triangle.branch"),
        ("Bookmarks", "bookmark"),
        ("Warnings", "exclamationmark.triangle"),
        ("Errors", "xmark.octagon"),
    ]

    var body: some View {
        HStack(spacing: 0) {
            ForEach(tabs.indices, id: \.self) { index in
                Button {
                    selection = index
                } label: {
                    Image(systemName: tabs[index].1)
                        .frame(maxWidth: .infinity)
                        .frame(height: 28)
                        .contentShape(Capsule())
                        .glassEffect(selection == index ? .regular.interactive() : .identity, in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(tabs[index].0)
                .accessibilityAddTraits(selection == index ? .isSelected : [])
                .help(tabs[index].0)
            }
        }
        .padding(3)
        .glassEffect(.regular, in: Capsule())
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Sidebar")
    }
}
