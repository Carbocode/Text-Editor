import AppKit
import SwiftUI

/// Dedicated closable document tabs; the editor stays outside the switching
/// labels so selection, focus and undo survive a rename.
struct DocumentTabs<Content: View>: View {
    let documents: [String]
    let dirtyDocuments: Set<String>
    let selectedDocument: String?
    let select: (String) -> Bool
    let close: (String) -> Void
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(spacing: 0) {
            if !documents.isEmpty {
                ScrollViewReader { proxy in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: BonsplitTabBarMetrics.tabSpacing) {
                            ForEach(documents, id: \.self) { id in
                                BonsplitTabItemView(
                                    tab: BonsplitTabItem(title: (id as NSString).lastPathComponent,
                                                        isDirty: dirtyDocuments.contains(id)),
                                    isSelected: selectedDocument == id,
                                    fileImage: WorkspaceIcons.image(for: id),
                                    accessibilityID: id,
                                    onSelect: { _ = select(id) },
                                    onClose: { close(id) })
                                .help(id)
                                .accessibilityIdentifier("documentTab:" + id)
                                .id(id)
                            }
                        }
                        .padding(.horizontal, BonsplitTabBarMetrics.barPadding)
                        .frame(height: BonsplitTabBarMetrics.barHeight)
                    }
                    .onAppear { if let selectedDocument { proxy.scrollTo(selectedDocument) } }
                    .onChange(of: selectedDocument) { _, id in
                        if let id { proxy.scrollTo(id, anchor: .center) }
                    }
                }
                .frame(height: BonsplitTabBarMetrics.barHeight)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("documentTabBar")
            }
            content()
        }
    }
}
