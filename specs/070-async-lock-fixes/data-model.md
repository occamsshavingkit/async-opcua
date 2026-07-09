# Data Model: Async Lock Audit Remediation

**Feature**: 070-async-lock-fixes | **Date**: 2026-07-07

This document describes concurrency state models for the systems being modified.

## 1. Lock Hierarchy (Post-Fix)
- SessionManager.write → Session.read/write (no nested locks)
- SessionManager.read → Session.read/write (sequential, not nested)
- SubscriptionCache.write → (no nested reads)
- SubscriptionCache.read → (flat, no nesting)
- Notify (channel renewal) → SecureChannel.read/write (parking_lot, dropped before .await)
- DashMap (continuation points) → Connection pool (r2d2)

## 2. Session close_session (TOCTOU-safe)
After fix: pre-compute `was_unactivated` flag under read lock, validate after write lock reacquisition.

## 3. Session activate_session (crypto offloaded)
After fix: extract signature data under locks, drop locks, RSA verify in spawn_blocking, then continue.

## 4. Subscription notification flow (waker-correct)
After fix: drain_ring → process_queued_publish_requests → PublishResponse sent immediately.

## 5. Client SubscriptionState (actor model)
After fix: message-passing replaces 22+ Mutex acquisitions. Zero lock acquisitions in hot path.

## 6. SQLite Pool
arc<Mutex<Connection>> → r2d2::Pool<SqliteConnectionManager>. WAL mode concurrent reads.

## 7. Client Offset (atomic fix)
load()+store() → rcu() atomic read-modify-write. No lost updates.

## 8. Browse Name Index (DCL)
Read-then-write with inner double-check under write lock.
