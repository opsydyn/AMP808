import Foundation

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
            selectedQueueAnchorIndex(
                knownSelectedQueueIndex: nil,
                queueCount: 2,
                currentQueueIndex: 0
            ),
            0,
            "a newly resolved current entry must become the selected queue anchor"
        )
        requireEqual(
            selectedQueueAnchorIndex(
                knownSelectedQueueIndex: nil,
                queueCount: 2,
                currentQueueIndex: 2
            ),
            nil,
            "an out-of-range current queue entry must fail closed"
        )
        requireEqual(
            selectedQueueAnchorIndex(
                knownSelectedQueueIndex: 1,
                queueCount: 2,
                currentQueueIndex: 0
            ),
            1,
            "a previously resolved queue anchor must remain authoritative"
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

    private static func requireEqual(_ actual: Int?, _ expected: Int?, _ message: String) {
        guard actual == expected else {
            fatalError("\(message): expected \(String(describing: expected)), got \(String(describing: actual))")
        }
    }
}
