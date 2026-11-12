import Foundation
import MusicKit

@main
private enum QueueOccurrenceMapTests {
    static func main() {
        requireEqual(
            orderedQueueOccurrenceMap(
                playlistCount: 5,
                queueCount: 3,
                selectedPlaylistIndex: 0,
                selectedQueueIndex: 0
            ),
            [0: 0, 1: 1, 2: 2],
            "a suffix queue must preserve playlist occurrence order"
        )
        requireEqual(
            orderedQueueOccurrenceMap(
                playlistCount: 4,
                queueCount: 4,
                selectedPlaylistIndex: 2,
                selectedQueueIndex: 2
            ),
            [0: 0, 1: 1, 2: 2, 3: 3],
            "a full queue must preserve duplicate occurrence positions"
        )
        requireEqual(
            orderedQueueOccurrenceMap(
                playlistCount: 2,
                queueCount: 3,
                selectedPlaylistIndex: 0,
                selectedQueueIndex: 0
            ),
            nil,
            "a queue extending beyond the loaded playlist must fail closed"
        )
        requireEqual(
            orderedQueueOccurrenceMap(
                playlistCount: 4,
                queueCount: 2,
                selectedPlaylistIndex: 2,
                selectedQueueIndex: 0
            ),
            [0: 2, 1: 3],
            "a queue beginning at the selected occurrence must map its tail"
        )
        requireEqual(
            queueEntryOccurrenceMap(
                entryIDs: ["queue-0", "queue-1", "queue-2", "queue-3"],
                playlistCount: 4,
                selectedPlaylistIndex: 2,
                selectedQueueEntryID: "queue-2"
            ),
            ["queue-0": 0, "queue-1": 1, "queue-2": 2, "queue-3": 3],
            "queue entry identity must preserve exact playlist occurrences"
        )
        requireEqual(
            queueEntryOccurrenceMap(
                entryIDs: ["queue-0", "queue-0"],
                playlistCount: 2,
                selectedPlaylistIndex: 0,
                selectedQueueEntryID: "queue-0"
            ),
            nil,
            "duplicate queue entry identity must fail closed"
        )
        requireEqual(
            queueEntryOccurrenceMap(
                entryIDs: ["queue-2", "queue-3"],
                playlistCount: 4,
                selectedPlaylistIndex: 2,
                selectedQueueEntryID: "queue-2"
            ),
            ["queue-2": 2, "queue-3": 3],
            "a suffix queue must retain playlist occurrence identity"
        )
        requireEqual(
            queueEntryOccurrenceMap(
                entryIDs: ["queue-0", "   "],
                playlistCount: 2,
                selectedPlaylistIndex: 0,
                selectedQueueEntryID: "queue-0"
            ),
            nil,
            "blank queue entry identity must fail closed"
        )
        requireEqual(
            queueEntryOccurrenceMap(
                entryIDs: ["queue-0", "queue-1"],
                playlistCount: 2,
                selectedPlaylistIndex: 0,
                selectedQueueEntryID: "queue-missing"
            ),
            nil,
            "a missing selected queue identity must fail closed"
        )
        let reorderedPlaylistIndices = queueEntryOccurrenceMap(
            entryIDs: ["queue-0", "queue-1", "queue-2", "queue-3"],
            playlistCount: 4,
            selectedPlaylistIndex: 2,
            selectedQueueEntryID: "queue-2"
        ).map { occurrenceMap in
            ["queue-3", "queue-0", "queue-2", "queue-1"].compactMap {
                occurrenceMap[$0]
            }
        }
        requireEqual(
            reorderedPlaylistIndices,
            [3, 0, 2, 1],
            "later queue order must not change an entry's playlist occurrence"
        )
        requireEqual(
            queueEntryOccurrenceMap(
                entryIDs: ["queue-0", "queue-1", "queue-2"],
                playlistCount: 2,
                selectedPlaylistIndex: 0,
                selectedQueueEntryID: "queue-0"
            ),
            nil,
            "a queue extending beyond the playlist must fail closed"
        )
        requireEqual(
            repeatModeForCommandValue(0),
            MusicPlayer.RepeatMode.none,
            "repeat value zero must disable repeat"
        )
        requireEqual(
            repeatModeForCommandValue(1),
            MusicPlayer.RepeatMode.all,
            "repeat value one must repeat the queue"
        )
        requireEqual(
            repeatModeForCommandValue(2),
            MusicPlayer.RepeatMode.one,
            "repeat value two must repeat the current entry"
        )
        requireEqual(
            repeatModeForCommandValue(3),
            nil,
            "unknown repeat values must fail closed"
        )
    }

    private static func requireEqual(
        _ actual: [Int: Int]?,
        _ expected: [Int: Int]?,
        _ message: String
    ) {
        guard actual == expected else {
            fatalError("\(message): expected \(String(describing: expected)), got \(String(describing: actual))")
        }
    }

    private static func requireEqual(_ actual: [Int]?, _ expected: [Int]?, _ message: String) {
        guard actual == expected else {
            fatalError("\(message): expected \(String(describing: expected)), got \(String(describing: actual))")
        }
    }

    private static func requireEqual(
        _ actual: [String: Int]?,
        _ expected: [String: Int]?,
        _ message: String
    ) {
        guard actual == expected else {
            fatalError("\(message): expected \(String(describing: expected)), got \(String(describing: actual))")
        }
    }

    private static func requireEqual(
        _ actual: MusicPlayer.RepeatMode?,
        _ expected: MusicPlayer.RepeatMode?,
        _ message: String
    ) {
        guard actual == expected else {
            fatalError("\(message): expected \(String(describing: expected)), got \(String(describing: actual))")
        }
    }
}
