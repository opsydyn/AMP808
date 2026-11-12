import Foundation
import MusicKit

private let envelopeVersion = 1
private let expectedBundleIdentifier = "com.opsydyn.amp808r3tr0"
private let playlistPageSize = 50
private let playlistTrackPageSize = 100

private struct PlaybackQueueContext: Sendable {
    let playlistID: String
    let playlistIndexByQueueEntryID: [String: Int]
}

func orderedQueueOccurrenceMap(
    playlistCount: Int,
    queueCount: Int,
    selectedPlaylistIndex: Int,
    selectedQueueIndex: Int
) -> [Int: Int]? {
    guard playlistCount > 0,
          queueCount > 0,
          (0..<playlistCount).contains(selectedPlaylistIndex),
          (0..<queueCount).contains(selectedQueueIndex) else {
        return nil
    }

    var mapping: [Int: Int] = [:]
    for queueIndex in 0..<queueCount {
        let relativeIndex = queueIndex - selectedQueueIndex
        let (playlistIndex, overflowed) = selectedPlaylistIndex.addingReportingOverflow(
            relativeIndex
        )
        guard !overflowed, (0..<playlistCount).contains(playlistIndex) else {
            return nil
        }
        mapping[queueIndex] = playlistIndex
    }
    return mapping
}

func queueEntryOccurrenceMap(
    entryIDs: [String],
    playlistCount: Int,
    selectedPlaylistIndex: Int,
    selectedQueueEntryID: String
) -> [String: Int]? {
    guard !entryIDs.isEmpty,
          entryIDs.allSatisfy({ !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }),
          Set(entryIDs).count == entryIDs.count,
          let selectedQueueIndex = entryIDs.firstIndex(of: selectedQueueEntryID),
          let playlistIndicesByQueueIndex = orderedQueueOccurrenceMap(
              playlistCount: playlistCount,
              queueCount: entryIDs.count,
              selectedPlaylistIndex: selectedPlaylistIndex,
              selectedQueueIndex: selectedQueueIndex
          ) else {
        return nil
    }

    var occurrenceByEntryID: [String: Int] = [:]
    for (queueIndex, entryID) in entryIDs.enumerated() {
        guard let playlistIndex = playlistIndicesByQueueIndex[queueIndex] else {
            return nil
        }
        occurrenceByEntryID[entryID] = playlistIndex
    }
    return occurrenceByEntryID
}

@available(macOS 14.0, *)
func repeatModeForCommandValue(_ value: UInt64) -> MusicPlayer.RepeatMode? {
    switch value {
    case 0:
        MusicPlayer.RepeatMode.none
    case 1:
        .all
    case 2:
        .one
    default:
        nil
    }
}

@MainActor private var playbackQueueContext: PlaybackQueueContext?

public typealias Amp808MusicKitConnectionCallback = @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafeMutablePointer<UInt8>?,
    UInt
) -> Void

private struct ConnectionCompletion: @unchecked Sendable {
    let context: UnsafeMutableRawPointer?
    let callback: Amp808MusicKitConnectionCallback

    func send(_ envelope: String) {
        let (pointer, length) = allocateEnvelope(envelope)
        callback(context, pointer, length)
    }
}

private struct PlaylistEnvelopeItem: Encodable {
    let id: String
    let name: String?
    let artworkURL: String?

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case artworkURL = "artwork_url"
    }
}

private struct PlaylistPageEnvelope: Encodable {
    let version: Int
    let kind: String
    let requestID: UInt64
    let requestedCursor: String?
    let nextCursor: String?
    let playlists: [PlaylistEnvelopeItem]
    let error: String?

    enum CodingKeys: String, CodingKey {
        case version
        case kind
        case requestID = "request_id"
        case requestedCursor = "requested_cursor"
        case nextCursor = "next_cursor"
        case playlists
        case error
    }
}

private struct TrackEnvelopeItem: Encodable {
    let id: String
    let title: String
    let artist: String
    let album: String?
    let durationMS: UInt64?

    enum CodingKeys: String, CodingKey {
        case id
        case title
        case artist
        case album
        case durationMS = "duration_ms"
    }
}

private struct TrackPageEnvelope: Encodable {
    let version: Int
    let kind: String
    let requestID: UInt64
    let playlistID: String
    let requestedCursor: String?
    let nextCursor: String?
    let tracks: [TrackEnvelopeItem]
    let error: String?

    enum CodingKeys: String, CodingKey {
        case version
        case kind
        case requestID = "request_id"
        case playlistID = "playlist_id"
        case requestedCursor = "requested_cursor"
        case nextCursor = "next_cursor"
        case tracks
        case error
    }
}

private struct PlaybackEnvelope: Encodable {
    let version: Int
    let kind: String
    let requestID: UInt64
    let trackID: String
    let status: String
    let error: String?

    enum CodingKeys: String, CodingKey {
        case version
        case kind
        case requestID = "request_id"
        case trackID = "track_id"
        case status
        case error
    }
}

private struct TransportEnvelope: Encodable {
    let version: Int
    let kind: String
    let requestID: UInt64
    let trackID: String
    let status: String
    let repeatMode: String?
    let error: String?

    init(
        version: Int,
        kind: String,
        requestID: UInt64,
        trackID: String,
        status: String,
        repeatMode: String? = nil,
        error: String?
    ) {
        self.version = version
        self.kind = kind
        self.requestID = requestID
        self.trackID = trackID
        self.status = status
        self.repeatMode = repeatMode
        self.error = error
    }

    enum CodingKeys: String, CodingKey {
        case version
        case kind
        case requestID = "request_id"
        case trackID = "track_id"
        case status
        case repeatMode = "repeat_mode"
        case error
    }
}

private struct PositionEnvelope: Encodable {
    let version: Int
    let kind: String
    let requestID: UInt64
    let playlistID: String
    let playlistIndex: UInt64
    let elapsedMS: UInt64
    let durationMS: UInt64?
    let playbackStatus: String?
    let error: String?

    enum CodingKeys: String, CodingKey {
        case version
        case kind
        case requestID = "request_id"
        case playlistID = "playlist_id"
        case playlistIndex = "playlist_index"
        case elapsedMS = "elapsed_ms"
        case durationMS = "duration_ms"
        case playbackStatus = "playback_status"
        case error
    }
}

private func sendPlaylistEnvelope(
    _ envelope: PlaylistPageEnvelope,
    completion: ConnectionCompletion
) {
    if let data = try? JSONEncoder().encode(envelope),
       let text = String(data: data, encoding: .utf8) {
        completion.send(text)
        return
    }

    completion.send(
        "{\"version\":\(envelopeVersion),\"kind\":\"playlist_page\",\"request_id\":\(envelope.requestID),\"requested_cursor\":null,\"next_cursor\":null,\"playlists\":[],\"error\":\"encoding_failed\"}"
    )
}

private func sendTrackEnvelope(
    _ envelope: TrackPageEnvelope,
    completion: ConnectionCompletion
) {
    if let data = try? JSONEncoder().encode(envelope),
       let text = String(data: data, encoding: .utf8) {
        completion.send(text)
        return
    }

    completion.send(
        "{\"version\":\(envelopeVersion),\"kind\":\"playlist_track_page\",\"request_id\":\(envelope.requestID),\"playlist_id\":\"\",\"requested_cursor\":null,\"next_cursor\":null,\"tracks\":[],\"error\":\"encoding_failed\"}"
    )
}

private func sendPlaybackEnvelope(
    _ envelope: PlaybackEnvelope,
    completion: ConnectionCompletion
) {
    if let data = try? JSONEncoder().encode(envelope),
       let text = String(data: data, encoding: .utf8) {
        completion.send(text)
        return
    }

    completion.send(
        "{\"version\":\(envelopeVersion),\"kind\":\"playback\",\"request_id\":\(envelope.requestID),\"track_id\":\"\",\"status\":\"failed\",\"error\":\"encoding_failed\"}"
    )
}

private func sendTransportEnvelope(
    _ envelope: TransportEnvelope,
    completion: ConnectionCompletion
) {
    if let data = try? JSONEncoder().encode(envelope),
       let text = String(data: data, encoding: .utf8) {
        completion.send(text)
        return
    }

    completion.send(
        "{\"version\":\(envelopeVersion),\"kind\":\"transport\",\"request_id\":\(envelope.requestID),\"track_id\":\"\",\"status\":\"failed\",\"error\":\"encoding_failed\"}"
    )
}

private func sendPositionEnvelope(
    _ envelope: PositionEnvelope,
    completion: ConnectionCompletion
) {
    if let data = try? JSONEncoder().encode(envelope),
       let text = String(data: data, encoding: .utf8) {
        completion.send(text)
        return
    }

    completion.send(
        "{\"version\":\(envelopeVersion),\"kind\":\"position\",\"request_id\":\(envelope.requestID),\"playlist_id\":\"\",\"playlist_index\":0,\"elapsed_ms\":0,\"duration_ms\":null,\"playback_status\":null,\"error\":\"encoding_failed\"}"
    )
}

private func durationMilliseconds(_ duration: TimeInterval?) -> UInt64? {
    guard let duration, duration.isFinite, duration >= 0 else {
        return nil
    }
    return UInt64((duration * 1_000).rounded())
}

@available(macOS 14.0, *)
private func queueEntryDurationMilliseconds(
    _ item: MusicPlayer.Queue.Entry.Item?
) -> UInt64? {
    switch item {
    case .song(let song):
        durationMilliseconds(song.duration)
    case .musicVideo(let musicVideo):
        durationMilliseconds(musicVideo.duration)
    case nil:
        nil
    @unknown default:
        nil
    }
}

@available(macOS 14.0, *)
private func playbackStatusLabel(_ status: MusicPlayer.PlaybackStatus) -> String {
    switch status {
    case .stopped:
        "stopped"
    case .playing:
        "playing"
    case .paused:
        "paused"
    case .interrupted:
        "interrupted"
    case .seekingForward:
        "seeking_forward"
    case .seekingBackward:
        "seeking_backward"
    @unknown default:
        "unknown"
    }
}

@available(macOS 14.0, *)
private func repeatModeLabel(_ mode: MusicPlayer.RepeatMode?) -> String {
    switch mode {
    case nil, .some(.none):
        "off"
    case .some(.all):
        "all"
    case .some(.one):
        "one"
    @unknown default:
        "unknown"
    }
}

private func httpsArtworkURL(_ artwork: Artwork?) -> String? {
    guard let url = artwork?.url(width: 300, height: 300),
          url.scheme?.lowercased() == "https",
          let host = url.host,
          !host.isEmpty else {
        return nil
    }
    return url.absoluteString
}

private func authorizationName(_ status: MusicAuthorization.Status) -> String {
    switch status {
    case .notDetermined: "not_determined"
    case .denied: "denied"
    case .restricted: "restricted"
    case .authorized: "authorised"
    @unknown default: "unknown"
    }
}

private func allocateEnvelope(_ envelope: String) -> (UnsafeMutablePointer<UInt8>, UInt) {
    let bytes = Array(envelope.utf8)
    let pointer = UnsafeMutablePointer<UInt8>.allocate(capacity: bytes.count)
    bytes.withUnsafeBufferPointer { source in
        guard let sourceAddress = source.baseAddress else {
            return
        }
        pointer.initialize(from: sourceAddress, count: source.count)
    }
    return (pointer, UInt(bytes.count))
}

@c(amp808_music_kit_capability_probe)
public func capabilityProbe(
    _ requestID: UInt64,
    _ outputLength: UnsafeMutablePointer<UInt>?
) -> UnsafeMutablePointer<UInt8>? {
    guard let outputLength else {
        return nil
    }
    outputLength.pointee = 0

    let authorisation = authorizationName(MusicAuthorization.currentStatus)
    let envelope = "{\"version\":\(envelopeVersion),\"kind\":\"capability\",\"request_id\":\(requestID),\"authorisation\":\"\(authorisation)\"}"
    let (pointer, length) = allocateEnvelope(envelope)
    outputLength.pointee = length
    return pointer
}

@c(amp808_music_kit_request_connection)
public func requestConnection(
    _ requestID: UInt64,
    _ context: UnsafeMutableRawPointer?,
    _ callback: Amp808MusicKitConnectionCallback?
) -> UInt8 {
    guard let callback else {
        return 0
    }
    guard let usageDescription = Bundle.main.object(
        forInfoDictionaryKey: "NSAppleMusicUsageDescription"
    ) as? String, !usageDescription.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 2
    }
    guard Bundle.main.bundleIdentifier == expectedBundleIdentifier else {
        return 3
    }
    let completion = ConnectionCompletion(context: context, callback: callback)

    Task {
        let status = await MusicAuthorization.request()
        let authorisation = authorizationName(status)
        guard status == .authorized else {
            completion.send("{\"version\":\(envelopeVersion),\"kind\":\"connection\",\"request_id\":\(requestID),\"authorisation\":\"\(authorisation)\",\"subscription\":\"not_applicable\",\"can_play_catalog_content\":null,\"can_become_subscriber\":null,\"has_cloud_library_enabled\":null}")
            return
        }

        do {
            let subscription = try await MusicSubscription.current
            completion.send("{\"version\":\(envelopeVersion),\"kind\":\"connection\",\"request_id\":\(requestID),\"authorisation\":\"authorised\",\"subscription\":\"available\",\"can_play_catalog_content\":\(subscription.canPlayCatalogContent),\"can_become_subscriber\":\(subscription.canBecomeSubscriber),\"has_cloud_library_enabled\":\(subscription.hasCloudLibraryEnabled)}")
        } catch {
            completion.send("{\"version\":\(envelopeVersion),\"kind\":\"connection\",\"request_id\":\(requestID),\"authorisation\":\"authorised\",\"subscription\":\"unavailable\",\"can_play_catalog_content\":null,\"can_become_subscriber\":null,\"has_cloud_library_enabled\":null}")
        }
    }
    return 1
}

@c(amp808_music_kit_request_playlist_page)
public func requestPlaylistPage(
    _ requestID: UInt64,
    _ offset: UInt64,
    _ context: UnsafeMutableRawPointer?,
    _ callback: Amp808MusicKitConnectionCallback?
) -> UInt8 {
    guard let callback else {
        return 0
    }
    guard let usageDescription = Bundle.main.object(
        forInfoDictionaryKey: "NSAppleMusicUsageDescription"
    ) as? String, !usageDescription.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 2
    }
    guard Bundle.main.bundleIdentifier == expectedBundleIdentifier else {
        return 3
    }
    guard offset <= UInt64(Int.max) else {
        return 4
    }
    let completion = ConnectionCompletion(context: context, callback: callback)

    Task {
        guard #available(macOS 14.0, *) else {
            sendPlaylistEnvelope(
                PlaylistPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_page",
                    requestID: requestID,
                    requestedCursor: offset == 0 ? nil : String(offset),
                    nextCursor: nil,
                    playlists: [],
                    error: "unsupported_system"
                ),
                completion: completion
            )
            return
        }
        guard MusicAuthorization.currentStatus == .authorized else {
            sendPlaylistEnvelope(
                PlaylistPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_page",
                    requestID: requestID,
                    requestedCursor: offset == 0 ? nil : String(offset),
                    nextCursor: nil,
                    playlists: [],
                    error: "access_denied"
                ),
                completion: completion
            )
            return
        }

        do {
            var request = MusicLibraryRequest<Playlist>()
            request.limit = playlistPageSize
            request.offset = Int(offset)
            let response = try await request.response()
            let playlists = response.items.map { playlist in
                PlaylistEnvelopeItem(
                    id: playlist.id.rawValue,
                    name: playlist.name,
                    artworkURL: httpsArtworkURL(playlist.artwork)
                )
            }
            let nextCursor = response.items.hasNextBatch
                ? String(offset + UInt64(response.items.count))
                : nil
            sendPlaylistEnvelope(
                PlaylistPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_page",
                    requestID: requestID,
                    requestedCursor: offset == 0 ? nil : String(offset),
                    nextCursor: nextCursor,
                    playlists: playlists,
                    error: nil
                ),
                completion: completion
            )
        } catch {
            sendPlaylistEnvelope(
                PlaylistPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_page",
                    requestID: requestID,
                    requestedCursor: offset == 0 ? nil : String(offset),
                    nextCursor: nil,
                    playlists: [],
                    error: "request_failed"
                ),
                completion: completion
            )
        }
    }
    return 1
}

@c(amp808_music_kit_request_track_page)
public func requestTrackPage(
    _ requestID: UInt64,
    _ playlistIDBytes: UnsafePointer<UInt8>?,
    _ playlistIDLength: UInt,
    _ offset: UInt64,
    _ context: UnsafeMutableRawPointer?,
    _ callback: Amp808MusicKitConnectionCallback?
) -> UInt8 {
    guard let callback else {
        return 0
    }
    guard let usageDescription = Bundle.main.object(
        forInfoDictionaryKey: "NSAppleMusicUsageDescription"
    ) as? String, !usageDescription.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 2
    }
    guard Bundle.main.bundleIdentifier == expectedBundleIdentifier else {
        return 3
    }
    guard offset <= UInt64(Int.max),
          playlistIDLength <= UInt(Int.max),
          let playlistIDBytes,
          let playlistID = String(
              bytes: UnsafeBufferPointer(
                  start: playlistIDBytes,
                  count: Int(playlistIDLength)
              ),
              encoding: .utf8
          ),
          !playlistID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 4
    }
    let completion = ConnectionCompletion(context: context, callback: callback)

    Task {
        let requestedCursor = offset == 0 ? nil : String(offset)
        guard #available(macOS 14.0, *) else {
            sendTrackEnvelope(
                TrackPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_track_page",
                    requestID: requestID,
                    playlistID: playlistID,
                    requestedCursor: requestedCursor,
                    nextCursor: nil,
                    tracks: [],
                    error: "unsupported_system"
                ),
                completion: completion
            )
            return
        }
        guard MusicAuthorization.currentStatus == .authorized else {
            sendTrackEnvelope(
                TrackPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_track_page",
                    requestID: requestID,
                    playlistID: playlistID,
                    requestedCursor: requestedCursor,
                    nextCursor: nil,
                    tracks: [],
                    error: "access_denied"
                ),
                completion: completion
            )
            return
        }

        do {
            var request = MusicLibraryRequest<Playlist>()
            request.limit = 1
            request.filter(matching: \.id, equalTo: MusicItemID(playlistID))
            let response = try await request.response()
            guard let playlist = response.items.first else {
                sendTrackEnvelope(
                    TrackPageEnvelope(
                        version: envelopeVersion,
                        kind: "playlist_track_page",
                        requestID: requestID,
                        playlistID: playlistID,
                        requestedCursor: requestedCursor,
                        nextCursor: nil,
                        tracks: [],
                        error: "playlist_not_found"
                    ),
                    completion: completion
                )
                return
            }

            let detailedPlaylist = try await playlist.with([.entries])
            var entries = detailedPlaylist.entries ?? []
            let requiredCount = Int(offset) + playlistTrackPageSize
            while entries.count < requiredCount, entries.hasNextBatch {
                guard let next = try await entries.nextBatch(limit: playlistTrackPageSize) else {
                    break
                }
                guard !next.isEmpty else {
                    break
                }
                entries += next
            }

            let start = min(Int(offset), entries.count)
            let end = min(start + playlistTrackPageSize, entries.count)
            let tracks = entries[start..<end].map { entry in
                TrackEnvelopeItem(
                    id: entry.id.rawValue,
                    title: entry.title,
                    artist: entry.artistName,
                    album: entry.albumTitle,
                    durationMS: durationMilliseconds(entry.duration)
                )
            }
            let hasMore = end < entries.count || entries.hasNextBatch
            sendTrackEnvelope(
                TrackPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_track_page",
                    requestID: requestID,
                    playlistID: playlistID,
                    requestedCursor: requestedCursor,
                    nextCursor: hasMore ? String(end) : nil,
                    tracks: tracks,
                    error: nil
                ),
                completion: completion
            )
        } catch {
            sendTrackEnvelope(
                TrackPageEnvelope(
                    version: envelopeVersion,
                    kind: "playlist_track_page",
                    requestID: requestID,
                    playlistID: playlistID,
                    requestedCursor: requestedCursor,
                    nextCursor: nil,
                    tracks: [],
                    error: "request_failed"
                ),
                completion: completion
            )
        }
    }
    return 1
}

@c(amp808_music_kit_play_track)
public func playTrack(
    _ requestID: UInt64,
    _ playlistIDBytes: UnsafePointer<UInt8>?,
    _ playlistIDLength: UInt,
    _ trackIDBytes: UnsafePointer<UInt8>?,
    _ trackIDLength: UInt,
    _ playlistIndexValue: UInt64,
    _ context: UnsafeMutableRawPointer?,
    _ callback: Amp808MusicKitConnectionCallback?
) -> UInt8 {
    guard let callback else {
        return 0
    }
    guard let usageDescription = Bundle.main.object(
        forInfoDictionaryKey: "NSAppleMusicUsageDescription"
    ) as? String, !usageDescription.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 2
    }
    guard Bundle.main.bundleIdentifier == expectedBundleIdentifier else {
        return 3
    }
    guard playlistIDLength <= UInt(Int.max),
          let playlistIDBytes,
          let playlistID = String(
              bytes: UnsafeBufferPointer(
                  start: playlistIDBytes,
                  count: Int(playlistIDLength)
              ),
              encoding: .utf8
          ),
          !playlistID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
          trackIDLength <= UInt(Int.max),
          let trackIDBytes,
          let trackID = String(
              bytes: UnsafeBufferPointer(
                  start: trackIDBytes,
                  count: Int(trackIDLength)
              ),
              encoding: .utf8
          ),
          !trackID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 4
    }
    guard let playlistIndex = Int(exactly: playlistIndexValue) else {
        return 4
    }
    let completion = ConnectionCompletion(context: context, callback: callback)

    Task {
        guard #available(macOS 14.0, *), MusicAuthorization.currentStatus == .authorized else {
            sendPlaybackEnvelope(
                PlaybackEnvelope(
                    version: envelopeVersion,
                    kind: "playback",
                    requestID: requestID,
                    trackID: trackID,
                    status: "failed",
                    error: "access_unavailable"
                ),
                completion: completion
            )
            return
        }

        do {
            var request = MusicLibraryRequest<Playlist>()
            request.limit = 1
            request.filter(matching: \.id, equalTo: MusicItemID(playlistID))
            let response = try await request.response()
            guard let playlist = response.items.first else {
                sendPlaybackEnvelope(
                    PlaybackEnvelope(
                        version: envelopeVersion,
                        kind: "playback",
                        requestID: requestID,
                        trackID: trackID,
                        status: "failed",
                        error: "track_not_found"
                    ),
                    completion: completion
                )
                return
            }

            let detailedPlaylist = try await playlist.with([.entries])
            var entries = detailedPlaylist.entries ?? []
            while entries.hasNextBatch {
                guard let next = try await entries.nextBatch(limit: playlistTrackPageSize) else {
                    break
                }
                guard !next.isEmpty else {
                    break
                }
                entries += next
            }
            guard entries.indices.contains(playlistIndex) else {
                sendPlaybackEnvelope(
                    PlaybackEnvelope(
                        version: envelopeVersion,
                        kind: "playback",
                        requestID: requestID,
                        trackID: trackID,
                        status: "failed",
                        error: "track_not_found"
                    ),
                    completion: completion
                )
                return
            }
            let entry = entries[playlistIndex]
            guard entry.id.rawValue == trackID else {
                sendPlaybackEnvelope(
                    PlaybackEnvelope(
                        version: envelopeVersion,
                        kind: "playback",
                        requestID: requestID,
                        trackID: trackID,
                        status: "failed",
                        error: "track_not_found"
                    ),
                    completion: completion
                )
                return
            }

            let player = ApplicationMusicPlayer.shared
            await MainActor.run {
                playbackQueueContext = nil
            }
            player.queue = ApplicationMusicPlayer.Queue(
                playlist: detailedPlaylist,
                startingAt: entry
            )
            try await player.prepareToPlay()
            try await player.play()

            let queueContext = player.queue.currentEntry.flatMap { selectedQueueEntry in
                queueEntryOccurrenceMap(
                    entryIDs: player.queue.entries.map(\.id),
                    playlistCount: entries.count,
                    selectedPlaylistIndex: playlistIndex,
                    selectedQueueEntryID: selectedQueueEntry.id
                ).map { playlistIndexByQueueEntryID in
                    PlaybackQueueContext(
                        playlistID: playlistID,
                        playlistIndexByQueueEntryID: playlistIndexByQueueEntryID
                    )
                }
            }
            await MainActor.run {
                playbackQueueContext = queueContext
            }

            sendPlaybackEnvelope(
                PlaybackEnvelope(
                    version: envelopeVersion,
                    kind: "playback",
                    requestID: requestID,
                    trackID: trackID,
                    status: "playing",
                    error: nil
                ),
                completion: completion
            )
        } catch {
            sendPlaybackEnvelope(
                PlaybackEnvelope(
                    version: envelopeVersion,
                    kind: "playback",
                    requestID: requestID,
                    trackID: trackID,
                    status: "failed",
                    error: "playback_failed"
                ),
                completion: completion
            )
        }
    }
    return 1
}

@c(amp808_music_kit_transport)
public func controlPlayback(
    _ requestID: UInt64,
    _ command: UInt8,
    _ commandValue: UInt64,
    _ trackIDBytes: UnsafePointer<UInt8>?,
    _ trackIDLength: UInt,
    _ context: UnsafeMutableRawPointer?,
    _ callback: Amp808MusicKitConnectionCallback?
) -> UInt8 {
    guard let callback else {
        return 0
    }
    guard let usageDescription = Bundle.main.object(
        forInfoDictionaryKey: "NSAppleMusicUsageDescription"
    ) as? String, !usageDescription.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 2
    }
    guard Bundle.main.bundleIdentifier == expectedBundleIdentifier else {
        return 3
    }
    guard command <= 6,
          command != 6 || commandValue <= 2 else {
        return 4
    }
    guard trackIDLength <= UInt(Int.max),
          let trackIDBytes,
          let trackID = String(
              bytes: UnsafeBufferPointer(
                  start: trackIDBytes,
                  count: Int(trackIDLength)
              ),
              encoding: .utf8
          ),
          !trackID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 4
    }
    let completion = ConnectionCompletion(context: context, callback: callback)

    Task {
        guard #available(macOS 14.0, *), MusicAuthorization.currentStatus == .authorized else {
            sendTransportEnvelope(
                TransportEnvelope(
                    version: envelopeVersion,
                    kind: "transport",
                    requestID: requestID,
                    trackID: trackID,
                    status: "failed",
                    error: "access_unavailable"
                ),
                completion: completion
            )
            return
        }

        let player = ApplicationMusicPlayer.shared
        switch command {
        case 0:
            player.pause()
            sendTransportEnvelope(
                TransportEnvelope(
                    version: envelopeVersion,
                    kind: "transport",
                    requestID: requestID,
                    trackID: trackID,
                    status: "paused",
                    error: nil
                ),
                completion: completion
            )
        case 1:
            do {
                try await player.play()
                sendTransportEnvelope(
                    TransportEnvelope(
                        version: envelopeVersion,
                        kind: "transport",
                        requestID: requestID,
                        trackID: trackID,
                        status: "playing",
                        error: nil
                    ),
                    completion: completion
                )
            } catch {
                sendTransportEnvelope(
                    TransportEnvelope(
                        version: envelopeVersion,
                        kind: "transport",
                        requestID: requestID,
                        trackID: trackID,
                        status: "failed",
                        error: "transport_failed"
                    ),
                    completion: completion
                )
            }
        case 2:
            player.stop()
            sendTransportEnvelope(
                TransportEnvelope(
                    version: envelopeVersion,
                    kind: "transport",
                    requestID: requestID,
                    trackID: trackID,
                    status: "stopped",
                    error: nil
                ),
                completion: completion
            )
        case 3:
            do {
                try await player.skipToNextEntry()
                sendTransportEnvelope(
                    TransportEnvelope(
                        version: envelopeVersion,
                        kind: "transport",
                        requestID: requestID,
                        trackID: trackID,
                        status: "playing",
                        error: nil
                    ),
                    completion: completion
                )
            } catch {
                sendTransportEnvelope(
                    TransportEnvelope(
                        version: envelopeVersion,
                        kind: "transport",
                        requestID: requestID,
                        trackID: trackID,
                        status: "failed",
                        error: "transport_failed"
                    ),
                    completion: completion
                )
            }
        case 4:
            do {
                try await player.skipToPreviousEntry()
                sendTransportEnvelope(
                    TransportEnvelope(
                        version: envelopeVersion,
                        kind: "transport",
                        requestID: requestID,
                        trackID: trackID,
                        status: "playing",
                        error: nil
                    ),
                    completion: completion
                )
            } catch {
                sendTransportEnvelope(
                    TransportEnvelope(
                        version: envelopeVersion,
                        kind: "transport",
                        requestID: requestID,
                        trackID: trackID,
                        status: "failed",
                        error: "transport_failed"
                    ),
                    completion: completion
                )
            }
        case 5:
            player.playbackTime = TimeInterval(commandValue) / 1_000
            sendTransportEnvelope(
                TransportEnvelope(
                    version: envelopeVersion,
                    kind: "transport",
                    requestID: requestID,
                    trackID: trackID,
                    status: "playing",
                    error: nil
                ),
                completion: completion
            )
        case 6:
            guard let repeatMode = repeatModeForCommandValue(commandValue) else {
                preconditionFailure("validated MusicKit repeat mode became invalid")
            }
            player.state.repeatMode = repeatMode
            sendTransportEnvelope(
                TransportEnvelope(
                    version: envelopeVersion,
                    kind: "transport",
                    requestID: requestID,
                    trackID: trackID,
                    status: playbackStatusLabel(player.state.playbackStatus),
                    repeatMode: repeatModeLabel(player.state.repeatMode),
                    error: nil
                ),
                completion: completion
            )
        default:
            preconditionFailure("validated MusicKit transport command became invalid")
        }
    }
    return 1
}

@c(amp808_music_kit_position)
public func observePlaybackPosition(
    _ requestID: UInt64,
    _ context: UnsafeMutableRawPointer?,
    _ callback: Amp808MusicKitConnectionCallback?
) -> UInt8 {
    guard let callback else {
        return 0
    }
    guard let usageDescription = Bundle.main.object(
        forInfoDictionaryKey: "NSAppleMusicUsageDescription"
    ) as? String, !usageDescription.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return 2
    }
    guard Bundle.main.bundleIdentifier == expectedBundleIdentifier else {
        return 3
    }
    let completion = ConnectionCompletion(context: context, callback: callback)

    Task { @MainActor in
        guard #available(macOS 14.0, *), MusicAuthorization.currentStatus == .authorized else {
            sendPositionEnvelope(
                PositionEnvelope(
                    version: envelopeVersion,
                    kind: "position",
                    requestID: requestID,
                    playlistID: "",
                    playlistIndex: 0,
                    elapsedMS: 0,
                    durationMS: nil,
                    playbackStatus: nil,
                    error: "access_unavailable"
                ),
                completion: completion
            )
            return
        }

        let playbackTime = ApplicationMusicPlayer.shared.playbackTime
        let milliseconds = playbackTime * 1_000
        let player = ApplicationMusicPlayer.shared
        guard let queueContext = playbackQueueContext,
              let currentEntry = player.queue.currentEntry,
              let playlistIndex = queueContext.playlistIndexByQueueEntryID[currentEntry.id] else {
            sendPositionEnvelope(
                PositionEnvelope(
                    version: envelopeVersion,
                    kind: "position",
                    requestID: requestID,
                    playlistID: "",
                    playlistIndex: 0,
                    elapsedMS: 0,
                    durationMS: nil,
                    playbackStatus: nil,
                    error: "position_unavailable"
                ),
                completion: completion
            )
            return
        }
        guard let encodedPlaylistIndex = UInt64(exactly: playlistIndex),
              milliseconds.isFinite,
              milliseconds >= 0,
              milliseconds < Double(UInt64.max) else {
            sendPositionEnvelope(
                PositionEnvelope(
                    version: envelopeVersion,
                    kind: "position",
                    requestID: requestID,
                    playlistID: queueContext.playlistID,
                    playlistIndex: 0,
                    elapsedMS: 0,
                    durationMS: nil,
                    playbackStatus: nil,
                    error: "position_unavailable"
                ),
                completion: completion
            )
            return
        }
        sendPositionEnvelope(
            PositionEnvelope(
                version: envelopeVersion,
                kind: "position",
                requestID: requestID,
                playlistID: queueContext.playlistID,
                playlistIndex: encodedPlaylistIndex,
                elapsedMS: UInt64(milliseconds.rounded(.down)),
                durationMS: queueEntryDurationMilliseconds(currentEntry.item),
                playbackStatus: playbackStatusLabel(player.state.playbackStatus),
                error: nil
            ),
            completion: completion
        )
    }
    return 1
}

@c(amp808_music_kit_release_buffer)
public func releaseBuffer(_ pointer: UnsafeMutablePointer<UInt8>?) {
    guard let pointer else {
        return
    }
    pointer.deallocate()
}
