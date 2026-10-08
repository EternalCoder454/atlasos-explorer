pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Files' Settings: one window with five short pages, the one home of every
// setting (the menus keep quick switches for the few that are used often).
// Simple first, details folded; every control has a one-line description.
// General, View, Search, Context Menu and Actions, Trash. The values live in
// the classes that keep them (SettingsLogic, TabLogic, ViewMemory, ServerLogic,
// ColumnLogic, PreviewLogic, TrashLogic, MenuPrefs, ActionsLogic); this window
// only reads and changes them. Opened from the View menu, the tab menu and
// Ctrl+, whatever the context menus hide.
TelamonPreferencesDialog {
    id: dlg

    // The window (Main.qml): the tab's view, and the functions that change it.
    required property var win

    // The editor of a custom action asks for `id` (0: a new action).
    signal editActionRequested(int id)
    signal removeActionRequested(int id)

    title: qsTr("Settings")
    preferredWidth: Kirigami.Units.gridUnit * 50
    stateKey: "files"

    // Shows the page `index` (0 General ... 4 Trash); with none, the page that was shown last.
    function showPage(index) {
        if (index !== undefined) {
            currentIndex = index;
        }
        open();
    }
    onAboutToShow: {
        SettingsLogic.refreshIndex();
        MenuPrefs.scan();
    }

    // The plain words for each built-in menu entry.
    readonly property var entryText: ({
            "open": [qsTr("Open"), qsTr("Opens the selected items.")],
            "restore": [qsTr("Restore"), qsTr("In the Trash: puts the items back where they were.")],
            "openWith": [qsTr("Open With"), qsTr("Opens the items with another application.")],
            "cut": [qsTr("Cut"), qsTr("Cuts the items, to move them with Paste.")],
            "copy": [qsTr("Copy"), qsTr("Copies the items.")],
            "paste": [qsTr("Paste"), qsTr("Pastes what was cut or copied.")],
            "rename": [qsTr("Rename"), qsTr("Renames an item, or several together.")],
            "trash": [qsTr("Move to Trash"), qsTr("Moves the items to the Trash.")],
            "extractHere": [qsTr("Extract Here"), qsTr("Unpacks an archive next to itself (with Telamon Archive).")],
            "extractTo": [qsTr("Extract To…"), qsTr("Unpacks an archive where you choose (with Telamon Archive).")],
            "compressZip": [qsTr("Compress to ZIP"), qsTr("Packs the items into a ZIP file (with Telamon Archive).")],
            "compress": [qsTr("Compress…"), qsTr("Packs the items into an archive of your choice (with Telamon Archive).")],
            "tags": [qsTr("Tags"), qsTr("Puts colour and name tags on the items.")],
            "properties": [qsTr("Properties"), qsTr("Shows details, permissions and checksums.")],
            "moreActions": [qsTr("More Actions"), qsTr("The submenu with the rarer entries, your own actions and the service menus.")],
            "openInNewTab": [qsTr("Open in New Tab"), qsTr("Opens a folder in a new tab.")],
            "openFileLocation": [qsTr("Open File Location"), qsTr("Shows the folder that holds a search result.")],
            "openTerminal": [qsTr("Open Terminal Here"), qsTr("Opens a terminal in the folder.")],
            "pinToSidebar": [qsTr("Pin to Sidebar"), qsTr("Adds a folder to the sidebar.")],
            "copyPath": [qsTr("Copy Path"), qsTr("Copies the full path of the items as text.")],
            "hide": [qsTr("Hide"), qsTr("Hides items by listing them in the folder's .hidden file.")],
            "unhide": [qsTr("Unhide"), qsTr("Takes items off the folder's .hidden file.")],
            "deleteForGood": [qsTr("Delete for Good…"), qsTr("Deletes the items without the Trash, after asking.")],
            "new": [qsTr("New"), qsTr("In empty space: makes a folder, a text file or one from your templates.")],
            "undo": [qsTr("Undo"), qsTr("In empty space: takes back the last change.")],
            "redo": [qsTr("Redo"), qsTr("In empty space: does the change that was taken back again.")],
            "sort": [qsTr("Sort"), qsTr("In empty space: the sort and group choices.")],
            "view": [qsTr("View"), qsTr("In empty space: the view choices.")],
            "pinFolder": [qsTr("Pin This Folder to Sidebar"), qsTr("In empty space: adds the folder to the sidebar.")],
            "rotateLeft": [qsTr("Rotate Left"), qsTr("Turns pictures a quarter turn to the left, into new files.")],
            "rotateRight": [qsTr("Rotate Right"), qsTr("Turns pictures a quarter turn to the right, into new files.")],
            "convertPng": [qsTr("Convert to PNG"), qsTr("Makes PNG copies of pictures.")],
            "convertJpeg": [qsTr("Convert to JPEG"), qsTr("Makes JPEG copies of pictures.")],
            "convertWebp": [qsTr("Convert to WebP"), qsTr("Makes WebP copies of pictures.")],
            "combinePdf": [qsTr("Combine into PDF"), qsTr("Joins pictures and PDFs into one PDF.")],
            "copyToOtherPane": [qsTr("Copy to Other Pane"), qsTr("With the view split: copies the items to the other pane.")],
            "moveToOtherPane": [qsTr("Move to Other Pane"), qsTr("With the view split: moves the items to the other pane.")]
        })

    // ---- General ----
    TelamonPreferencesPage {
        title: qsTr("General")
        symbol: Symbols.Settings

        Section {
            title: qsTr("Starting")
            TelamonFormEntry {
                label: qsTr("New tabs open at")
                help: qsTr("What a new tab, and Files itself, show first.")
                TelamonSegmentedControl {
                    model: [qsTr("Home Page"), qsTr("Home Folder")]
                    currentIndex: SettingsLogic.startPage
                    onActivated: index => SettingsLogic.startPage = index
                }
            }
            TelamonFormEntry {
                label: qsTr("Restore tabs on start")
                help: qsTr("Open the tabs you had when you closed Files.")
                TelamonSwitch {
                    checked: dlg.win.restoreTabs
                    onToggled: dlg.win.setRestoreTabs(checked)
                }
            }
        }
        Section {
            title: qsTr("Files and Folders")
            TelamonFormEntry {
                label: qsTr("Show hidden files")
                help: qsTr("Show files and folders whose names start with a dot (Ctrl+H).")
                TelamonSwitch {
                    checked: dlg.win.view ? dlg.win.view.folder.showHidden : false
                    onToggled: dlg.win.toggleHidden()
                }
            }
        }
    }

    // ---- View ----
    TelamonPreferencesPage {
        title: qsTr("View")
        symbol: Symbols.ViewList

        Section {
            title: qsTr("Folders")
            TelamonFormEntry {
                label: qsTr("Use the same view for every folder")
                help: qsTr("Off: each folder remembers its own view, sort order and icon size.")
                TelamonSwitch {
                    checked: ViewMemory.sameForAll
                    onToggled: dlg.win.setSameView(checked)
                }
            }
            TelamonFormEntry {
                label: qsTr("Show the preview pane")
                help: qsTr("A side pane with a large preview and the details of the selected item (Alt+P).")
                TelamonSwitch {
                    checked: PreviewLogic.paneShown
                    onToggled: PreviewLogic.paneShown = checked
                }
            }
            TelamonFormEntry {
                label: qsTr("Preview files on servers")
                help: qsTr("Thumbnails and previews of files on a server download them first, so this is off until you turn it on.")
                TelamonSwitch {
                    checked: ServerLogic.previewRemote
                    onToggled: dlg.win.setPreviewRemote(checked)
                }
            }
            TelamonFormEntry {
                label: qsTr("Show Git status")
                help: qsTr("Mark modified, new and ignored items in a Git folder. Files runs git in the folder to find out; off by default.")
                TelamonSwitch {
                    checked: SettingsLogic.gitBadges
                    onToggled: SettingsLogic.gitBadges = checked
                }
            }
        }
        TelamonExpandableSection {
            Layout.fillWidth: true
            title: qsTr("Details Columns")
            subtitle: qsTr("Extra columns of the Details view")
            Section {
                Layout.fillWidth: true
                TelamonFormEntry {
                    label: qsTr("Tags")
                    help: qsTr("The names of an item's tags.")
                    TelamonSwitch {
                        checked: ColumnLogic.tags
                        onToggled: ColumnLogic.tags = checked
                    }
                }
                TelamonFormEntry {
                    label: qsTr("Dimensions")
                    help: qsTr("The size in pixels of pictures and videos.")
                    TelamonSwitch {
                        checked: ColumnLogic.dimensions
                        onToggled: ColumnLogic.dimensions = checked
                    }
                }
                TelamonFormEntry {
                    label: qsTr("Duration")
                    help: qsTr("How long music and videos play.")
                    TelamonSwitch {
                        checked: ColumnLogic.duration
                        onToggled: ColumnLogic.duration = checked
                    }
                }
                TelamonFormEntry {
                    label: qsTr("Date taken")
                    help: qsTr("When a photo was taken.")
                    TelamonSwitch {
                        checked: ColumnLogic.taken
                        onToggled: ColumnLogic.taken = checked
                    }
                }
            }
        }
    }

    // ---- Search ----
    TelamonPreferencesPage {
        title: qsTr("Search")
        symbol: Symbols.Search

        Section {
            title: qsTr("Searching")
            TelamonFormEntry {
                label: qsTr("Use patterns by default")
                help: qsTr("New tabs and panes treat the words typed in the search and filter fields as regular expressions, like .*\\.png$.")
                TelamonSwitch {
                    checked: SettingsLogic.usePattern
                    onToggled: SettingsLogic.usePattern = checked
                }
            }
        }
        TelamonExpandableSection {
            Layout.fillWidth: true
            title: qsTr("File Index")
            subtitle: SettingsLogic.indexStatus
            Section {
                Layout.fillWidth: true
                footer: qsTr("The index remembers the names of the files in these folders, so a search answers at once. It never reads what is inside the files.")
                TelamonFormEntry {
                    label: qsTr("Rebuild the index")
                    help: qsTr("Look at the folders again now, after many files changed outside Files.")
                    TelamonButton {
                        text: qsTr("Rebuild")
                        symbol: Symbols.Sync
                        enabled: !SettingsLogic.indexOff
                        onClicked: SettingsLogic.rebuildIndex()
                    }
                }
                Repeater {
                    model: SettingsLogic.indexFolders
                    delegate: TelamonFormEntry {
                        id: folderEntry
                        required property string modelData
                        label: SettingsLogic.folderLabel(modelData)
                        help: qsTr("A folder the index holds.")
                        TelamonButton {
                            text: qsTr("Remove")
                            symbol: Symbols.Remove
                            Accessible.name: qsTr("Remove %1 from the index").arg(folderEntry.label)
                            onClicked: dlg.indexProblem = SettingsLogic.removeIndexFolder(folderEntry.modelData)
                        }
                    }
                }
                TelamonFormEntry {
                    label: qsTr("Add a folder")
                    help: qsTr("Choose a folder, or type its path, then press Add.")
                    errorText: dlg.indexProblem
                    RowLayout {
                        spacing: TelamonStyle.spacingLarge
                        TelamonFolderField {
                            id: addFolder
                            Layout.fillWidth: true
                            placeholderText: qsTr("Choose a folder")
                            Accessible.name: qsTr("Folder to add")
                            onEdited: dlg.indexProblem = ""
                        }
                        TelamonButton {
                            text: qsTr("Add")
                            symbol: Symbols.Add
                            enabled: addFolder.path.length > 0
                            onClicked: {
                                dlg.indexProblem = SettingsLogic.addIndexFolder(addFolder.path);
                                if (dlg.indexProblem.length === 0) {
                                    addFolder.path = "";
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- Context Menu and Actions ----
    TelamonPreferencesPage {
        title: qsTr("Context Menu and Actions")
        symbol: Symbols.Menu

        Section {
            title: qsTr("Your Actions")
            footer: qsTr("An action runs a program on the selected files. It appears under More Actions. Files starts the program directly, never through a shell, and each file name stays one argument.")
            Repeater {
                model: ActionsLogic.items
                delegate: SectionRow {
                    id: actionRow
                    required property var modelData
                    title: modelData.name
                    subtitle: modelData.command
                    // The names and commands are the user's own text, but shown as plain text all the same.
                    Accessible.name: modelData.name
                    Accessible.description: modelData.command
                    TelamonButton {
                        text: qsTr("Edit")
                        symbol: Symbols.Edit
                        Accessible.name: qsTr("Edit %1").arg(actionRow.modelData.name)
                        onClicked: dlg.editActionRequested(actionRow.modelData.id)
                    }
                    TelamonButton {
                        text: qsTr("Remove")
                        symbol: Symbols.Delete
                        Accessible.name: qsTr("Remove %1").arg(actionRow.modelData.name)
                        onClicked: dlg.removeActionRequested(actionRow.modelData.id)
                    }
                }
            }
            SectionRow {
                visible: ActionsLogic.count === 0
                title: qsTr("No actions yet")
                subtitle: qsTr("Add one to run your own program from the right-click menu.")
            }
            SectionRow {
                title: qsTr("Add an Action")
                subtitle: ActionsLogic.count >= ActionsLogic.maxActions ? qsTr("That is as many as Files can hold.") : qsTr("Name a program, its arguments and the files it is for.")
                TelamonButton {
                    text: qsTr("Add Action…")
                    symbol: Symbols.Add
                    enabled: ActionsLogic.count < ActionsLogic.maxActions
                    onClicked: dlg.editActionRequested(0)
                }
            }
        }
        TelamonExpandableSection {
            Layout.fillWidth: true
            title: qsTr("Menu Entries")
            subtitle: qsTr("Choose what the right-click menus show")
            Section {
                Layout.fillWidth: true
                footer: qsTr("An entry that is off is left out of the menus. Turn it on again here whenever you like.")
                Repeater {
                    model: MenuPrefs.builtinKeys
                    delegate: TelamonFormEntry {
                        id: builtinEntry
                        required property string modelData
                        readonly property var words: dlg.entryText[modelData] ?? [modelData, ""]
                        label: words[0]
                        help: words[1]
                        TelamonSwitch {
                            checked: !MenuPrefs.isHidden(builtinEntry.modelData) && MenuPrefs.revision >= 0
                            onToggled: MenuPrefs.setHidden(builtinEntry.modelData, !checked)
                        }
                    }
                }
            }
        }
        TelamonExpandableSection {
            Layout.fillWidth: true
            title: qsTr("Service Menus and Plugins")
            subtitle: MenuPrefs.scanning ? qsTr("Looking…") : qsTr("%n found", "", MenuPrefs.services.length)
            Section {
                Layout.fillWidth: true
                footer: qsTr("Programs you installed add these to the menu. An entry that is off is left out.")
                SectionRow {
                    visible: !MenuPrefs.scanning && MenuPrefs.services.length === 0
                    title: qsTr("None found")
                    subtitle: qsTr("No program has added entries to the menu.")
                }
                Repeater {
                    model: MenuPrefs.services
                    delegate: TelamonFormEntry {
                        id: serviceEntry
                        required property var modelData
                        label: modelData.text
                        help: modelData.kind === "plugin" ? qsTr("A plugin: %1").arg(modelData.detail) : qsTr("From %1").arg(modelData.detail)
                        TelamonSwitch {
                            checked: !MenuPrefs.isHidden(serviceEntry.modelData.key) && MenuPrefs.revision >= 0
                            onToggled: MenuPrefs.setHidden(serviceEntry.modelData.key, !checked)
                        }
                    }
                }
            }
        }
    }

    // ---- Trash ----
    TelamonPreferencesPage {
        title: qsTr("Trash")
        symbol: Symbols.Delete

        Section {
            title: qsTr("Emptying")
            TelamonFormEntry {
                label: qsTr("Empty old items automatically")
                help: qsTr("Deletes, for good, what has been in the Trash longer than this when Files starts and once a day. Newer items are never touched.")
                TrashAutoEmpty {}
            }
        }
    }

    // What the last try to change the index's folders said.
    property string indexProblem
}
