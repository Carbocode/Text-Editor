// Adapted from Bonsplit. Copyright (c) 2026 Alasdair Monk.
// MIT license and pinned source revision: ../ThirdParty/Bonsplit/README.md
import AppKit
import SwiftUI

struct BonsplitTabItem {
    let title: String
    var icon: String? = "doc.text"
    var isDirty: Bool = false
}


/// Individual tab view with icon, title, close button, and dirty indicator
struct BonsplitTabItemView: View {
    let tab: BonsplitTabItem
    let isSelected: Bool
    var fileImage: NSImage? = nil
    var accessibilityID: String = ""
    let onSelect: () -> Void
    let onClose: () -> Void

    @State private var isHovered = false
    @State private var isCloseHovered = false

    var body: some View {
        HStack(spacing: BonsplitTabBarMetrics.contentSpacing) {
            // Icon
            if let fileImage {
                Image(nsImage: fileImage)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: BonsplitTabBarMetrics.iconSize, height: BonsplitTabBarMetrics.iconSize)
                    .accessibilityHidden(true)
            } else if let iconName = tab.icon {
                Image(systemName: iconName)
                    .font(.system(size: BonsplitTabBarMetrics.iconSize))
                    .foregroundStyle(isSelected ? BonsplitTabBarColors.activeText : BonsplitTabBarColors.inactiveText)
            }

            // Title
            Text(tab.title)
                .font(.system(size: BonsplitTabBarMetrics.titleFontSize))
                .lineLimit(1)
                .foregroundStyle(isSelected ? BonsplitTabBarColors.activeText : BonsplitTabBarColors.inactiveText)

            Spacer(minLength: 4)
            closeButton
        }
        .padding(.horizontal, BonsplitTabBarMetrics.tabHorizontalPadding)
        .frame(
            minWidth: BonsplitTabBarMetrics.tabMinWidth,
            maxWidth: BonsplitTabBarMetrics.tabMaxWidth,
            minHeight: BonsplitTabBarMetrics.tabHeight,
            maxHeight: BonsplitTabBarMetrics.tabHeight
        )
        // Keep foreground glyphs out of the glass render group. Rebuilding an
        // interactive glass subtree on hover can temporarily drop text/images.
        .background {
            if isSelected {
                tabShape.fill(Color.accentColor)
            } else {
                tabShape.fill(.clear)
                    .glassEffect(.regular, in: tabShape)
            }
        }
        .overlay(alignment: .leading) {
            if tab.isDirty {
                Circle()
                    .fill(isSelected ? BonsplitTabBarColors.activeText : BonsplitTabBarColors.inactiveText)
                    .frame(width: BonsplitTabBarMetrics.dirtyIndicatorSize,
                           height: BonsplitTabBarMetrics.dirtyIndicatorSize)
                    .padding(.leading, 4)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
        }
        .contentShape(tabShape)
        .onTapGesture {
            onSelect()
        }
        .onHover { isHovered = $0 }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(tab.title)
        .accessibilityAction { onSelect() }
        .accessibilityValue(tab.isDirty ? "Modified" : "")
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
    }

    private var tabShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: BonsplitTabBarMetrics.tabCornerRadius, style: .continuous)
    }

    private var closeButton: some View {
        Button(action: onClose) {
            Image(systemName: "xmark")
                .font(.system(size: BonsplitTabBarMetrics.closeIconSize, weight: .semibold))
                .foregroundStyle(isSelected ? BonsplitTabBarColors.activeText : BonsplitTabBarColors.inactiveText)
                .frame(width: BonsplitTabBarMetrics.closeButtonSize, height: BonsplitTabBarMetrics.closeButtonSize)
                .background {
                    Circle().fill(isCloseHovered
                        ? (isSelected ? Color.white.opacity(0.18) : Color.primary.opacity(0.08)) : .clear)
                }
        }
        .buttonStyle(.plain)
        .opacity(isSelected || isHovered || isCloseHovered ? 1 : 0)
        .allowsHitTesting(isSelected || isHovered || isCloseHovered)
        .accessibilityLabel("Close " + tab.title)
        .accessibilityIdentifier("closeDocumentTab:" + accessibilityID)
        .onHover { isCloseHovered = $0 }
    }
}

/// Foreground colors for solid accent selection and system glass.
enum BonsplitTabBarColors {
    static var activeText: Color { Color(nsColor: .selectedMenuItemTextColor) }
    static var inactiveText: Color { .primary }
}

/// Shared row sizing keeps the sidebar and document controls vertically centered.
enum BonsplitTabBarMetrics {
    static let barHeight: CGFloat = 50
    static let barPadding: CGFloat = 10
    static let tabHeight: CGFloat = 34
    static let tabMinWidth: CGFloat = 140
    static let tabMaxWidth: CGFloat = 220
    static let tabCornerRadius: CGFloat = 12
    static let tabHorizontalPadding: CGFloat = 12
    static let tabSpacing: CGFloat = 8
    static let iconSize: CGFloat = 14
    static let titleFontSize: CGFloat = 12
    static let closeButtonSize: CGFloat = 18
    static let closeIconSize: CGFloat = 9
    static let dirtyIndicatorSize: CGFloat = 5
    static let contentSpacing: CGFloat = 6
}
