import Foundation
import MusicKit

private let envelopeVersion = 1
private let expectedBundleIdentifier = "com.opsydyn.amp808r3tr0"
private let playlistPageSize = 50

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

@c(amp808_music_kit_release_buffer)
public func releaseBuffer(_ pointer: UnsafeMutablePointer<UInt8>?) {
    guard let pointer else {
        return
    }
    pointer.deallocate()
}
