import SwiftUI

enum SidebarSection: Int, CaseIterable {
    case files, search, git, bookmarks, warnings, errors
    var hasTree: Bool { self == .files || self == .search }
}

struct SidebarTabBar: View {
    @Binding var selection: SidebarSection

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
                    selection = SidebarSection.allCases[index]
                } label: {
                    Image(systemName: tabs[index].1)
                        .frame(maxWidth: .infinity)
                        .frame(height: 28)
                        .contentShape(Capsule())
                        .glassEffect(selection.rawValue == index ? .regular.interactive() : .identity, in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(tabs[index].0)
                .accessibilityIdentifier("sidebarTab:\(SidebarSection.allCases[index])")
                .accessibilityAddTraits(selection.rawValue == index ? .isSelected : [])
                .help(tabs[index].0)
            }
        }
        .padding(3)
        .glassEffect(.regular, in: Capsule())
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Sidebar")
        .accessibilityIdentifier("sidebarTabBar")
    }
}
