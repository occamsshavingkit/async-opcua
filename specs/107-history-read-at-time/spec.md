# Feature Specification: Historical ReadAtTimeDetails

**Feature Branch**: `107-history-read-at-time`
**Created**: 2026-07-18
**Status**: Draft
**Input**: User description: "Historical ReadAtTimeDetails: implement HistoryRead's ReadAtTimeDetails (OPC UA Part 11 v1.05.04 section 6.5.5, "Read at time functionality"), closing CU 3020 and evaluating CU 2991 as a dependent."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Client reads historical values at arbitrary timestamps (Priority: P1)

An operator or monitoring application wants the value a historized variable had at a specific
point in time -- for example, to correlate a process variable against an external event log --
without needing to know or request every raw sample the server happens to have recorded. The
requested timestamps do not need to match any actual recorded sample time.

**Why this priority**: This is the entire scope of the feature -- OPC UA's `ReadAtTimeDetails`
request exists specifically for this "value at time T" use case, distinct from raw-range reads
(which return every sample in a window) and processed/aggregate reads (which summarize a window).
Every other behavior in this spec exists to make this single client-facing capability correct.

**Independent Test**: Record a handful of raw values for a variable at known timestamps, then
issue a HistoryRead request asking for values at timestamps that (a) exactly match a recorded
sample, (b) fall between two recorded samples, and (c) fall outside the recorded range entirely.
Confirm each of the three cases returns the behavior described in the acceptance scenarios below,
independent of any other history feature.

**Acceptance Scenarios**:

1. **Given** a variable has a raw historized value recorded at exactly timestamp T, **When** a
   client requests a value at time T, **Then** the server returns that exact recorded value,
   marked as coming from a raw (not computed) source.
2. **Given** a variable has raw historized values recorded before and after a requested timestamp
   T (with no exact match at T) and the request does not ask for simple bounds, **When** the
   client requests a value at time T, **Then** the server returns a value computed by
   interpolating between the nearest usable recorded values on each side, marked as computed
   (not raw).
3. **Given** the same setup as scenario 2 but the request asks for simple bounds, **When** the
   client requests a value at time T, **Then** the server computes the value using only the
   immediately adjacent recorded samples (even if one of them is itself of poor quality), rather
   than searching further afield for a better-quality sample.
4. **Given** a requested timestamp falls outside any usable recorded history for a variable (for
   example, before the first ever recorded sample), **When** the client requests a value at that
   time, **Then** the server indicates no data is available for that specific timestamp, without
   failing the rest of the request.
5. **Given** a client requests multiple timestamps in a single call, some matching recorded
   samples, some requiring interpolation, and some outside the recorded range, **When** the
   request is processed, **Then** the server returns one independent result per requested
   timestamp, each reflecting its own case from scenarios 1-4.
6. **Given** a client requests this capability for a node or timestamp configuration the server
   does not support, **When** the request is processed, **Then** the server rejects that specific
   result with a clear "not supported" indication rather than an exact value.

---

### Edge Cases

- **Duplicate or out-of-order requested timestamps in one request**: each requested timestamp is
  evaluated independently and returned in the same order it was requested, regardless of ordering
  or duplication.
- **A variable with no historized data at all**: every requested timestamp for that variable
  returns "no data available," not a hard failure of the whole request.
- **A very large number of requested timestamps in one call**: the server may return only a
  prefix of the requested timestamps' results in one response, together with a continuation
  token the client uses to resume with the remaining requested timestamps on a subsequent call --
  the standard OPC UA continuation-point mechanism, applied here per-timestamp-index rather than
  per-time-range.
- **Immediately-adjacent recorded sample is itself of poor quality, under simple-bounds mode**:
  the server does not substitute a better-quality sample from further away -- it returns "no data
  available" for that specific timestamp, since simple-bounds mode's entire purpose is to use
  only the nearest recorded points as-is.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST support requesting historical values for a historized variable at
  one or more arbitrary timestamps in a single request, returning one independent result per
  requested timestamp.
- **FR-002**: For any requested timestamp that exactly matches a recorded raw sample's timestamp,
  the system MUST return that exact recorded value, marked so a client can tell it came directly
  from a recorded sample rather than being computed.
- **FR-003**: For any requested timestamp with no exact recorded match, the system MUST compute a
  value by interpolating between the nearest usable recorded values before and after that
  timestamp, marked so a client can tell it was computed rather than directly recorded.
- **FR-004**: When a request asks for simple-bounds interpolation, the system MUST compute using
  only the immediately adjacent recorded samples (regardless of their own quality), returning "no
  data available" for that timestamp if an immediately adjacent sample is itself of poor quality
  or does not exist.
- **FR-005**: When a request does not ask for simple-bounds interpolation, the system MUST search
  outward for the nearest usable-quality recorded sample on each side of the requested timestamp,
  returning "no data available" for that timestamp only if no usable sample exists on a required
  side.
- **FR-006**: The system MUST reject with a clear "not supported" indication any requested result
  whose timestamp-reporting configuration the target node does not support, without affecting the
  other independently-requested timestamps in the same call.
- **FR-007**: This capability MUST behave identically regardless of which history storage backend
  is configured, requiring no additional backend-specific configuration from the client.
- **FR-008**: This capability MUST be available for historized values of any supported historical
  data type -- including non-numeric/structured values -- to at least the same extent that raw
  historical reads of that type are already supported, with no regression to existing raw-read
  behavior for those types.

### Key Entities

- **Requested Timestamp**: a single point in time, supplied by the client as part of a batch, for
  which a historical value is wanted.
- **Historical Value Result**: the value returned for one requested timestamp, tagged with
  whether it came directly from a recorded sample, was computed by interpolation, or could not be
  determined at all.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A client can retrieve a historically meaningful value for any timestamp within a
  variable's recorded history window -- whether or not that exact timestamp was ever recorded --
  in one request covering multiple timestamps at once.
- **SC-002**: A value returned for a timestamp that exactly matches a recorded sample is
  identical to what a direct raw-history read of that same sample would return.
- **SC-003**: A request for a timestamp outside any usable recorded history returns a clear
  "no data" indication for that timestamp specifically, rather than failing the entire request or
  silently returning a fabricated value.
- **SC-004**: This capability behaves the same way across every history storage option this SDK
  ships, so a client's request does not need to change based on which backend is deployed.

## Assumptions

- Continuation points are genuinely supported (the real Part 11 spec text is explicit: "The
  standard ContinuationPoint rules (see 6.3) apply" to this request), paginating over the client's
  requested-timestamp array by index rather than over a time range the way raw/processed reads
  do.
- "Computed" (interpolated) values follow this server's existing Interpolated Aggregate
  computation rules, since OPC UA's own specification for this feature explicitly directs
  implementers to reuse those same rules rather than defining a separate interpolation method.
- Closing the structured-data (non-numeric) case of this capability (CU 2991) is expected as a
  natural byproduct of a type-agnostic implementation; if investigation during planning shows it
  needs materially more work than that, this feature will document the remaining gap rather than
  build extra machinery speculatively to close it now.
