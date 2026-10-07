import QtQuick
import Telamon.Ui

// A place that isn't built yet, or a message in place of one: a symbol, a
// heading and a line of plain text (it may hold what a launch asked for).
TelamonPage {
    id: page

    property int symbol: Symbols.Construction
    property string heading
    property string text

    TelamonEmptyState {
        symbol: page.symbol
        title: page.heading
        text: page.text
    }
}
