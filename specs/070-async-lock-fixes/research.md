# Research: Async Lock Audit Remediation

**Feature**: 070-async-lock-fixes | **Date**: 2026-07-07
**Source**: Audit report at `specs/069-companion-specs/audit/report.md`

## R1: Crypto Offloading
Wrap RSA/ECC operations in `tokio::task::spawn_blocking`, cloning `Arc<>` handles into the closure. The SQLite backend already uses this pattern at 10 call sites.

## R2: SQLite Connection Pool
Use `r2d2-sqlite` with WAL mode. Default pool: 4 connections. Inner Mutex becomes redundant.

## R3: Session Expiry
Use `BinaryHeap<(Reverse<Instant>, NodeId)>` for O(log n) expiry instead of O(n) iteration.

## R4: Channel Renewal Deadlock Fix
Replace `tokio::sync::Mutex` with `AtomicBool` CAS + `Notify` + `RenewGuard` drop guard for cancellation-safe single-flight renewal.

## R5: Browse Name Index
Add double-check locking: re-check `is_none()` under write lock to prevent redundant index builds.

## R6: Client SubscriptionState Migration
Convert to actor model with `mpsc::unbounded_channel`. Eliminates all 22-27 Mutex acquisition sites.
