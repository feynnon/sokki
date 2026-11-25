// The Mac calendar through EventKit, for meeting-mail. See calendar.h.

import EventKit
import Foundation

private func out(_ s: String) -> UnsafeMutablePointer<CChar> {
    return strdup(s)!
}

private func statusName() -> String {
    let status = EKEventStore.authorizationStatus(for: .event)
    if #available(macOS 14.0, *) {
        switch status {
        case .fullAccess: return "allowed"
        case .writeOnly: return "write_only"
        default: break
        }
    }
    switch status {
    case .notDetermined: return "not_determined"
    case .restricted: return "restricted"
    case .denied: return "denied"
    default:
        // `.authorized` before macOS 14.
        return "allowed"
    }
}

@_cdecl("sokki_calendar_status")
public func sokki_calendar_status() -> UnsafeMutablePointer<CChar> {
    return out(statusName())
}

@_cdecl("sokki_calendar_request")
public func sokki_calendar_request() -> Bool {
    if statusName() == "allowed" { return true }
    let store = EKEventStore()
    let done = DispatchSemaphore(value: 0)
    var granted = false
    if #available(macOS 14.0, *) {
        store.requestFullAccessToEvents { ok, _ in
            granted = ok
            done.signal()
        }
    } else {
        store.requestAccess(to: .event) { ok, _ in
            granted = ok
            done.signal()
        }
    }
    done.wait()
    return granted
}

private func statusText(_ s: EKParticipantStatus) -> String {
    switch s {
    case .pending: return "pending"
    case .accepted: return "accepted"
    case .declined: return "declined"
    case .tentative: return "tentative"
    case .delegated: return "delegated"
    case .completed: return "completed"
    case .inProcess: return "in_process"
    default: return "unknown"
    }
}

private func kindText(_ t: EKParticipantType) -> String {
    switch t {
    case .person: return "person"
    case .room: return "room"
    case .resource: return "resource"
    case .group: return "group"
    default: return "unknown"
    }
}

private func participant(_ p: EKParticipant) -> [String: Any] {
    var d: [String: Any] = [
        "email": p.url.absoluteString,
        "is_me": p.isCurrentUser,
        "status": statusText(p.participantStatus),
        "kind": kindText(p.participantType),
    ]
    if let name = p.name { d["name"] = name }
    return d
}

@_cdecl("sokki_calendar_events")
public func sokki_calendar_events(_ from: Double, _ to: Double) -> UnsafeMutablePointer<CChar> {
    guard statusName() == "allowed" else { return out("[]") }
    // A new store each time: one made before access was granted sees nothing.
    let store = EKEventStore()
    let predicate = store.predicateForEvents(
        withStart: Date(timeIntervalSince1970: from),
        end: Date(timeIntervalSince1970: to),
        calendars: nil)
    var list: [[String: Any]] = []
    for e in store.events(matching: predicate) {
        var d: [String: Any] = [
            "title": e.title ?? "",
            "start": e.startDate.timeIntervalSince1970,
            "end": e.endDate.timeIntervalSince1970,
            "all_day": e.isAllDay,
            "attendees": (e.attendees ?? []).map(participant),
        ]
        if let url = e.url { d["url"] = url.absoluteString }
        if let location = e.location { d["location"] = location }
        if let notes = e.notes { d["notes"] = notes }
        if let organizer = e.organizer { d["organizer"] = participant(organizer) }
        if let source = e.calendar?.source?.title { d["account"] = source }
        list.append(d)
    }
    guard let data = try? JSONSerialization.data(withJSONObject: list),
          let text = String(data: data, encoding: .utf8)
    else { return out("[]") }
    return out(text)
}

@_cdecl("sokki_calendar_free")
public func sokki_calendar_free(_ s: UnsafeMutablePointer<CChar>?) {
    free(s)
}
