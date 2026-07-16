//! CU-indexed coverage reporting for OPC UA Foundation profile snapshots.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// Normalized OPC UA Foundation profile snapshot fields used by the report.
#[derive(Debug, Deserialize)]
pub struct NormalizedSnapshot {
    canonical_profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    profiles: Vec<Profile>,
    conformance_units: Vec<ConformanceUnit>,
    relationships: Relationships,
}

/// Profile metadata (canonical composite profiles and individual facets alike).
#[derive(Debug, Deserialize)]
pub struct Profile {
    display_name: String,
    opc_id: u32,
    opc_profile_uri: String,
}

/// Conformance Unit metadata.
#[derive(Debug, Deserialize)]
pub struct ConformanceUnit {
    name: String,
    opc_id: u32,
}

#[derive(Debug, Deserialize, Default)]
struct Relationships {
    /// Conformance units a profile directly requires.
    #[serde(default)]
    included_conformance_units: BTreeMap<String, Vec<u32>>,
    /// Other profiles a profile includes (recursively expanded to compute a
    /// full transitive CU closure). Composite profiles such as the four
    /// canonical 2025 server profiles are built almost entirely from this —
    /// most of their CUs come from included facets, not direct references.
    #[serde(default)]
    included_profiles: BTreeMap<String, Vec<u32>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum EvidenceStatus {
    Implemented,
    Partial,
    Gap,
    NeedsProof,
    SourceIssue,
    Extensible,
}

impl EvidenceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Partial => "partial",
            Self::Gap => "gap",
            Self::NeedsProof => "needs-proof",
            Self::SourceIssue => "source-issue",
            Self::Extensible => "extensible",
        }
    }
}

/// Parses the normalized Foundation profile snapshot JSON.
pub fn parse_snapshot(input: &str) -> Result<NormalizedSnapshot, serde_json::Error> {
    serde_json::from_str(input)
}

/// Computes the full transitive CU closure for `profile_id`: its own directly
/// included conformance units, plus those of every profile it includes,
/// recursively. Cycle-safe (a profile already on the current recursion stack
/// contributes nothing further, rather than recursing forever).
fn transitive_closure(
    profile_id: u32,
    relationships: &Relationships,
    memo: &mut BTreeMap<u32, BTreeSet<u32>>,
    stack: &mut BTreeSet<u32>,
) -> BTreeSet<u32> {
    if let Some(cached) = memo.get(&profile_id) {
        return cached.clone();
    }
    if !stack.insert(profile_id) {
        return BTreeSet::new();
    }

    let key = profile_id.to_string();
    let mut cus: BTreeSet<u32> = relationships
        .included_conformance_units
        .get(&key)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    if let Some(children) = relationships.included_profiles.get(&key) {
        for &child in children {
            cus.extend(transitive_closure(child, relationships, memo, stack));
        }
    }

    stack.remove(&profile_id);
    memo.insert(profile_id, cus.clone());
    cus
}

/// Generates a conservative CU-indexed Markdown report covering the four
/// canonical composite server profiles, every other individually-selectable
/// server facet in the snapshot, and a deduplicated ledger of every CU any of
/// them reference.
///
/// # Errors
///
/// Returns `Err` if `snapshot.relationships` has neither
/// `included_conformance_units` nor `included_profiles` populated. This
/// tool's transitive-closure computation depends entirely on those two
/// maps; a snapshot in the older shape (only a pre-computed
/// `transitive_cu_closure`, which this tool no longer reads) would
/// otherwise silently parse successfully and produce a report with every
/// closure empty, rather than a clear error.
pub fn generate_markdown_report(snapshot: &NormalizedSnapshot) -> Result<String, String> {
    if snapshot.relationships.included_conformance_units.is_empty()
        && snapshot.relationships.included_profiles.is_empty()
    {
        return Err(
            "snapshot.relationships has neither `included_conformance_units` nor \
             `included_profiles` populated. This tool requires that shape; the older \
             `transitive_cu_closure`-only shape is no longer supported. Regenerate the \
             snapshot with the current extractor."
                .to_string(),
        );
    }

    let conformance_units = snapshot
        .conformance_units
        .iter()
        .map(|unit| (unit.opc_id, unit.name.as_str()))
        .collect::<BTreeMap<_, _>>();

    let mut memo: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let closure_of = |id: u32, memo: &mut BTreeMap<u32, BTreeSet<u32>>| -> BTreeSet<u32> {
        let mut stack = BTreeSet::new();
        transitive_closure(id, &snapshot.relationships, memo, &mut stack)
    };

    let mut output = String::from("# OPC UA Foundation CU Coverage Report\n\n");
    output.push_str(
        "Status labels are evidence categories, not certification claims. Evidence for\n",
    );
    output
        .push_str("`implemented`/`partial`/`gap` entries comes from a 2026-07-15 code audit (7\n");
    output.push_str("independent passes over the codebase, one per subsystem cluster); see the\n");
    output
        .push_str("`Evidence` column for the specific file:line citation behind each verdict.\n\n");

    output.push_str("## Canonical Server Profiles\n\n");
    for tier in ["nano", "micro", "embedded", "standard"] {
        let Some(profile) = snapshot.canonical_profiles.get(tier) else {
            continue;
        };
        let closure = closure_of(profile.opc_id, &mut memo);

        output.push_str("### ");
        output.push_str(&profile.display_name);
        output.push_str("\n\n");
        output.push_str("- OPC profile id: `");
        output.push_str(&profile.opc_id.to_string());
        output.push_str("`\n");
        output.push_str("- URI: `");
        output.push_str(&profile.opc_profile_uri);
        output.push_str("`\n");
        output.push_str("- CU closure size: ");
        output.push_str(&closure.len().to_string());
        output.push_str("\n\n");
        write_cu_table(&mut output, &closure, &conformance_units);
    }

    // Every non-canonical profile (individually-selectable facets: A&C
    // variants, Historical Access variants, GDS, Data Access, Auditing,
    // User Token/Role, and so on). These are real, addressable server-side
    // conformance surfaces the four composite profiles above don't fully
    // enumerate on their own. The canonical profiles also appear in the flat
    // `profiles` list (they're profiles too), so they're excluded here to
    // avoid listing them twice.
    let canonical_ids: BTreeSet<u32> = snapshot
        .canonical_profiles
        .values()
        .map(|p| p.opc_id)
        .collect();
    let mut facets: Vec<&Profile> = snapshot
        .profiles
        .iter()
        .filter(|p| !canonical_ids.contains(&p.opc_id))
        .collect();
    facets.sort_by_key(|p| p.display_name.clone());

    output.push_str("## Additional Server Facets (Summary)\n\n");
    output.push_str(
        "One row per facet not already covered by the four canonical profiles above. \
         Counts are per-status within that facet's own CU closure; a CU counted here may \
         also appear in another facet or in the Full CU Ledger below.\n\n",
    );
    output.push_str("| Facet | OPC id | Closure | Implemented | Partial | Gap | Needs-proof | Extensible | Source-issue |\n");
    output.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for facet in &facets {
        let closure = closure_of(facet.opc_id, &mut memo);
        if closure.is_empty() {
            continue;
        }
        let mut counts = [0usize; 6];
        for cu_id in &closure {
            let status = match conformance_units.get(cu_id) {
                Some(_) => classify_cu(*cu_id),
                None => EvidenceStatus::SourceIssue,
            };
            let idx = match status {
                EvidenceStatus::Implemented => 0,
                EvidenceStatus::Partial => 1,
                EvidenceStatus::Gap => 2,
                EvidenceStatus::NeedsProof => 3,
                EvidenceStatus::Extensible => 4,
                EvidenceStatus::SourceIssue => 5,
            };
            counts[idx] += 1;
        }
        output.push_str("| ");
        output.push_str(&markdown_cell(&facet.display_name));
        output.push_str(" | ");
        output.push_str(&facet.opc_id.to_string());
        output.push_str(" | ");
        output.push_str(&closure.len().to_string());
        for count in counts {
            output.push_str(" | ");
            output.push_str(&count.to_string());
        }
        output.push_str(" |\n");
    }
    output.push('\n');

    // Deduplicated ledger of every CU referenced by any profile above (the
    // four canonical profiles plus every additional facet) — the master
    // reference for prioritization, independent of how many facets happen to
    // reference a given CU.
    let mut all_cus: BTreeSet<u32> = BTreeSet::new();
    for profile in snapshot
        .canonical_profiles
        .values()
        .chain(facets.iter().copied())
    {
        all_cus.extend(closure_of(profile.opc_id, &mut memo));
    }

    output.push_str("## Full CU Ledger\n\n");
    output.push_str(&format!(
        "{} distinct CUs referenced by any server profile or facet in this snapshot.\n\n",
        all_cus.len()
    ));
    write_cu_table(&mut output, &all_cus, &conformance_units);

    Ok(output)
}

fn write_cu_table(
    output: &mut String,
    closure: &BTreeSet<u32>,
    conformance_units: &BTreeMap<u32, &str>,
) {
    output.push_str("| CU | Name | Status | Evidence |\n");
    output.push_str("|---:|---|---|---|\n");
    for cu_id in closure {
        let (name, status) = match conformance_units.get(cu_id) {
            Some(name) => (*name, classify_cu(*cu_id)),
            None => (
                "Missing from normalized CU list",
                EvidenceStatus::SourceIssue,
            ),
        };
        output.push_str("| ");
        output.push_str(&cu_id.to_string());
        output.push_str(" | ");
        output.push_str(&markdown_cell(name));
        output.push_str(" | ");
        output.push_str(status.as_str());
        output.push_str(" | ");
        output.push_str(&evidence_note(*cu_id, status));
        output.push_str(" |\n");
    }
    output.push('\n');
}

fn markdown_cell(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace(['\u{2013}', '\u{2014}'], "-")
        .replace('\u{2019}', "'")
}

fn classify_cu(cu_id: u32) -> EvidenceStatus {
    // Feature 093: PTP/gPTP/NTP are a deliberate, documented user-supplied
    // extension point (docs/time-synchronization.md), not a generic partial
    // implementation — this takes precedence over the general audit table,
    // which has no way to represent that distinction.
    if extensible_cus().contains(&cu_id) {
        return EvidenceStatus::Extensible;
    }
    if let Ok(idx) = AUDIT_TABLE.binary_search_by_key(&cu_id, |(id, _, _)| *id) {
        return AUDIT_TABLE[idx].1;
    }
    // CUs reviewed before the 2026-07-15 audit that fall outside the 76
    // server-profile facets it covered (e.g. canonical-profile-only CUs).
    if legacy_implemented_cus().contains(&cu_id) {
        return EvidenceStatus::Implemented;
    }
    EvidenceStatus::NeedsProof
}

fn evidence_note(cu_id: u32, status: EvidenceStatus) -> String {
    if !extensible_cus().contains(&cu_id) {
        if let Ok(idx) = AUDIT_TABLE.binary_search_by_key(&cu_id, |(id, _, _)| *id) {
            return AUDIT_TABLE[idx].2.to_string();
        }
    }
    match status {
        EvidenceStatus::Implemented => {
            "Reviewed pre-2026-07-15-audit; existing tests/docs provide direct evidence."
                .to_string()
        }
        EvidenceStatus::Extensible => {
            "Satisfiable via user-supplied TimeSyncSource; documented extension point, not implemented in-library (feature 093).".to_string()
        }
        EvidenceStatus::SourceIssue => {
            "Referenced by closure but absent from conformance_units.".to_string()
        }
        EvidenceStatus::NeedsProof => {
            "Outside the 2026-07-15 audit's scope; not yet reviewed.".to_string()
        }
        EvidenceStatus::Partial | EvidenceStatus::Gap => {
            "Not found in the audit table (unexpected); treat as unreviewed.".to_string()
        }
    }
}

fn extensible_cus() -> BTreeSet<u32> {
    [2479, 2480, 2786].into_iter().collect()
}

/// CUs reviewed and confirmed implemented before the 2026-07-15 audit, that
/// fall outside the 76 server-profile facets it covered.
fn legacy_implemented_cus() -> BTreeSet<u32> {
    [3721, 3923, 5814].into_iter().collect()
}

/// 2026-07-15 codebase audit: (CU id, status, evidence). Sorted by CU id for
/// binary search. Produced by 7 independent research passes, one per
/// subsystem cluster (Alarms & Conditions, Historical Access, Subscriptions,
/// GDS/Security, User/Role/Auditing, Node/Type/Method, infrastructure/misc),
/// each verifying against actual code and tests rather than prior summaries.
static AUDIT_TABLE: &[(u32, EvidenceStatus, &str)] = &[
    (2163, EvidenceStatus::Gap, "UserWriteMask is a static field (nodes/base.rs:73,117), never per-user computed unlike UserAccessLevel (utils.rs:414); no test."),
    (2165, EvidenceStatus::Implemented, "agg_annotation_count engine.rs:1084 (id 2351); test aggregates_tests.rs:758,771-776"),
    (2166, EvidenceStatus::Implemented, "agg_maximum2 engine.rs:905 (id 11287); test aggregates_tests.rs:580-598 (Maximum2=20)"),
    (2175, EvidenceStatus::Implemented, "engine.rs dispatch_aggregate AGG_MINIMUM_ACTUAL_TIME2=11305 (aggregates/engine.rs:1487-1489); test aggregates_tests.rs:605 phase_d_min_actual_time2_uses_bound_timestamp"),
    (2178, EvidenceStatus::Implemented, "agg_variance_population engine.rs:1045 (11429); test phase_b_variance_and_stddev aggregates_tests.rs:369-393"),
    (2180, EvidenceStatus::Implemented, "Respond via dialog.rs:190-198 + methods.rs:301-317; tested alarms.rs:1725 dialog_condition_respond_ends_dialog_and_validates"),
    (2184, EvidenceStatus::Implemented, "agg_total2 engine.rs:733 (11304); test phase_d_time_average2_total2 aggregates_tests.rs:621"),
    (2185, EvidenceStatus::Gap, "data_history.rs:305-308 update_structure_data rejects non-Annotation ExtensionObjects (BadTypeMismatch); same in sqlite backend.rs:1030; test history_data_inmemory.rs:421 proves only Annotation type accepted, contra CU's exclusion of annotation-only support"),
    (2188, EvidenceStatus::Implemented, "engine.rs dispatch AGG_MAXIMUM2=11287 (engine.rs:1483); aggregates_tests.rs references 11287 (id present, phase_d family covers Minimum2/Maximum2 pattern)"),
    (2189, EvidenceStatus::Gap, "AlarmEvent (opcua-core/src/events.rs:9-35) has no condition-class field; BaseEventType.condition_class_id never set by alarms code (0 hits)"),
    (2194, EvidenceStatus::Implemented, "agg_delta_bounds engine.rs:1329 (11507); test phase_c_start_end_delta_bounds aggregates_tests.rs:486-503"),
    (2201, EvidenceStatus::Implemented, "agg_worst_quality engine.rs:1247 (2364); test aggregates_tests.rs:401, worst_quality_is_value_type_independent:1141"),
    (2202, EvidenceStatus::Implemented, "ConditionType_Enable/Disable Methods registered (methods.rs register_condition_methods); handle_condition_enable/disable call set_enabled; test alarms.rs::enable_disable_methods_toggle_enabled_state"),
    (2203, EvidenceStatus::Partial, "write_node_value accepts any Variant (address_space/utils.rs:473) but no test writes a structured/ExtensionObject value; only Read tested."),
    (2207, EvidenceStatus::Implemented, "agg_end_bound engine.rs:1321 (11506); test phase_c_start_end_delta_bounds aggregates_tests.rs:486-503"),
    (2210, EvidenceStatus::Implemented, "engine.rs dispatch AGG_TOTAL2=11304 (engine.rs:1486); test aggregates_tests.rs:621 phase_d_time_average2_total2_match_stepped_area"),
    (2220, EvidenceStatus::Implemented, "engine.rs dispatch AGG_DURATION_IN_STATE_ZERO=11307 (engine.rs:1504-1506); test aggregates_tests.rs:1169 duration_in_state_boolean_splits_false_and_true"),
    (2223, EvidenceStatus::Implemented, "engine.rs dispatch AGG_DURATION_IN_STATE_NON_ZERO=11308 (engine.rs:1507-1509); test aggregates_tests.rs:1169,1187 duration_in_state_* tests"),
    (2224, EvidenceStatus::Implemented, "event_history.rs:195-201 update_event PerformUpdateType::Replace; tests history_events_inmemory.rs:91 + sqlite history_events.rs:60 update_event_insert_replace_and_read"),
    (2231, EvidenceStatus::Partial, "Push StartSigningRequest+CreateSigningRequest impl gds/push_methods.rs:119,143 tested :435-552; missing GetTrustList/AddCert push ops"),
    (2232, EvidenceStatus::Gap, "Directory RegisterApplication/QueryServers unimplemented: method.rs:98-104,131-135 maps to BadServiceUnsupported; no callbacks registered"),
    (2233, EvidenceStatus::Gap, "Searched \"LDS-ME\",\"LdsMe\",\"lds_me\" - only unrelated mdns.rs hits; no GDS-to-LDS-ME semi-automatic registration config found"),
    (2236, EvidenceStatus::Gap, "CertificateExpirationAlarmType only structural (node_ids.rs:10623); no instantiation in async-opcua-server/src/alarms"),
    (2239, EvidenceStatus::Gap, "SystemOffNormalAlarmType only structural (node_ids.rs:10610); no instantiation in async-opcua-server/src/alarms"),
    (2256, EvidenceStatus::Implemented, "agg_delta engine.rs:776 (2359); test phase_b_count_average_range_delta aggregates_tests.rs:320-339"),
    (2258, EvidenceStatus::Gap, "Searched \"redundancy\"/\"RedundancySupport\"/\"redundant_server\" — only generated DataType stub (redundant_server_data_type.rs); no failover/clustering logic anywhere."),
    (2263, EvidenceStatus::Implemented, "engine.rs dispatch AGG_COUNT=2352 (engine.rs:1494); tests aggregates_tests.rs:1003-1059 count_* family (3 tests)"),
    (2264, EvidenceStatus::Implemented, "data_history.rs:239-252 update_data PerformUpdateType::Replace; tests history_data_inmemory.rs:79-91 + hda.rs:353-389 e2e_replace_then_read_modified"),
    (2267, EvidenceStatus::Implemented, "engine.rs dispatch AGG_START_BOUND=11505 (engine.rs:1510); test aggregates_tests.rs:486 part13_start_end_and_delta_bounds_use_simple_bounds"),
    (2273, EvidenceStatus::Implemented, "engine.rs dispatch AGG_TIME_AVERAGE2=11285 (engine.rs:1481); test aggregates_tests.rs:621 phase_d_time_average2_total2_match_stepped_area"),
    (2275, EvidenceStatus::Partial, "discrete.rs:22,182-186 implements Trip via DiscreteAlarmKind::Trip; grep shows Trip kind never used in any test (only OffNormal is)"),
    (2276, EvidenceStatus::Implemented, "annotations.rs attach_annotations_property + data_history.rs update_structure_data/read_annotations; simple.rs:658-718 history_read_annotations; test history_data_inmemory.rs:368 round-trip insert/replace/remove/read. Uses ReadAnnotationDataDetails not ReadRawModifiedDetails, but OPC-10000-11 5.1.2 confirms both are spec-valid"),
    (2281, EvidenceStatus::Implemented, "agg_variance_sample engine.rs:1021 (11428); test phase_b_variance_and_stddev aggregates_tests.rs:369-393"),
    (2282, EvidenceStatus::Implemented, "engine.rs dispatch AGG_END_BOUND=11506 (engine.rs:1511); test aggregates_tests.rs:486 part13_start_end_and_delta_bounds_use_simple_bounds"),
    (2289, EvidenceStatus::Partial, "event_history.rs:202-211 implements PerformUpdateType::Update (upsert) match arm; no dedicated test exercises Update mode for events (history_events_inmemory.rs + sqlite history_events.rs only test Insert/Replace)"),
    (2291, EvidenceStatus::Implemented, "custom_types.rs test_data_type_tree_builder reads a DynamicStructure e2e (tests/integration/custom_types.rs:61)."),
    (2302, EvidenceStatus::Implemented, "agg_minimum2 engine.rs:809 (11286); test phase_d_minimum2_includes_simple_bound aggregates_tests.rs:580"),
    (2303, EvidenceStatus::Implemented, "engine.rs dispatch AGG_PERCENT_GOOD=2362 (engine.rs:1501); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad"),
    (2305, EvidenceStatus::Implemented, "engine.rs dispatch AGG_TIME_AVERAGE=2343 (engine.rs:1474); test aggregates_tests.rs:203 test_calculate_aggregate_average + phase_c/d family"),
    (2309, EvidenceStatus::Implemented, "event_history.rs:188-194 update_event PerformUpdateType::Insert; tests history_events_inmemory.rs:56 + sqlite history_events.rs:60"),
    (2314, EvidenceStatus::Implemented, "engine.rs dispatch AGG_DURATION_BAD=2361 (engine.rs:1500); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad"),
    (2315, EvidenceStatus::Implemented, "handle_condition_refresh2 methods.rs:369-382; tested alarms.rs:584 condition_refresh2_targets_a_single_monitored_item"),
    (2317, EvidenceStatus::Implemented, "TranslateBrowsePathsToNodeIds handler async-opcua-server/src/session/services/view.rs:388; test async-opcua/tests/integration/tier_a.rs:141"),
    (2318, EvidenceStatus::Partial, "Clamp (monitored_item.rs:314-336 sanitize_queue_size) caps queuesize to max but 0 dedicated test; comment admits event handling is \"Future\""),
    (2319, EvidenceStatus::Implemented, "ServerBuilder certificate_path/private_key_path (builder.rs:359-366), pki_dir (builder.rs:494-495); tested security_tests.rs:421-568."),
    (2323, EvidenceStatus::Gap, "ExclusiveRateOfChangeAlarmType only structural (node_ids.rs:10588); no server instantiation"),
    (2328, EvidenceStatus::Implemented, "get_endpoints_with_filters incl profile-uri filter info.rs:342-378; tests core_tests.rs:100,358,366"),
    (2330, EvidenceStatus::Implemented, "agg_start_bound engine.rs:1289 (11505); test aggregates_tests.rs:486, part13_start_end_and_delta_bounds:1342"),
    (2332, EvidenceStatus::Gap, "data_history.rs read_raw_modified (lines 131-180) reads only the raw_values map; annotation_values (structured data) is a separate store never reachable via ReadRawModifiedDetails for any NodeId"),
    (2333, EvidenceStatus::Implemented, "create_in_address_space inserts Object+state vars (state_machine.rs:97-377); demo-server/alarms.rs uses it; test limit.rs:1093"),
    (2335, EvidenceStatus::Implemented, "engine.rs dispatch AGG_DELTA=2359 (engine.rs:1498); test aggregates_tests.rs:320 phase_b_count_average_range_delta"),
    (2338, EvidenceStatus::Gap, "only the standard Part 13 aggregate set (35 IDs, engine.rs supported_aggregates()/dispatch_aggregate) is implemented; default match arm (engine.rs:1521) returns BadAggregateNotSupported; no vendor/custom aggregate function found anywhere in aggregates/ module"),
    (2339, EvidenceStatus::Gap, "AggregateFunction_Start (i=2357 per schemas/1.05/NodeIds.csv:991) is absent from engine.rs AGG_ constants and dispatch_aggregate match (searched, no hit); only the distinct StartBound(11505) aggregate is implemented"),
    (2343, EvidenceStatus::Implemented, "Branch/create_branch/ack_branch/confirm_branch (state_machine.rs:9-28,396-452); test alarms.rs:1384 condition_branch_preserves_unacked"),
    (2345, EvidenceStatus::Implemented, "encode_value_as_xml/json (async-opcua-nodes/src/variable.rs:322); tests value_encodes_structure_as_default_xml/json (variable.rs:1165)."),
    (2346, EvidenceStatus::Implemented, "engine.rs dispatch AGG_MINIMUM=2346 (engine.rs:1476); test aggregates_tests.rs:228 test_calculate_aggregate_min_max"),
    (2350, EvidenceStatus::Implemented, "engine.rs dispatch AGG_DELTA_BOUNDS=11507 (engine.rs:1512); tests aggregates_tests.rs:486 phase_c_start_end_delta_bounds, :1342 part13_*"),
    (2352, EvidenceStatus::Implemented, "FindServers handled async-opcua-server/src/session/controller.rs:716; tests async-opcua/tests/integration/discovery.rs:83,119"),
    (2353, EvidenceStatus::Implemented, "TransferSubscriptions handler subscriptions/mod.rs:1671-1787, dispatched message_handler.rs:368; e2e async-opcua/tests/integration/subscriptions.rs:632,790."),
    (2354, EvidenceStatus::Gap, "Only inbound RegisterServer (LDS role, info.rs) + self-published discovery_urls found; no outbound \"register self with external Discovery Server URL\" config or disable switch."),
    (2358, EvidenceStatus::Implemented, "agg_std_dev_sample engine.rs:975; calculate_std_dev_sample math tested aggregates_tests.rs:173-182"),
    (2361, EvidenceStatus::Gap, "Searched 'TwoStateDiscreteType' repo-wide (excl generated nodeset) - zero instance code or tests."),
    (2362, EvidenceStatus::Implemented, "Method Nodes pervasive via MethodBuilder (async-opcua-nodes/src/method.rs); tested async-opcua/tests/integration/methods.rs."),
    (2371, EvidenceStatus::Implemented, "Hello/Ack+TCP codec async-opcua-core/src/comms/tcp_types.rs:244,373; exercised by full opc.tcp integration suite"),
    (2375, EvidenceStatus::Implemented, "agg_average engine.rs:684 (id 2342); test phase_b_count_average_range_delta aggregates_tests.rs:320-333"),
    (2376, EvidenceStatus::Implemented, "agg_minimum engine.rs:792 (2346); test test_calculate_aggregate_min_max aggregates_tests.rs:228"),
    (2377, EvidenceStatus::Implemented, "agg_range engine.rs:738 (2350); test phase_b_count_average_range_delta aggregates_tests.rs:320-336"),
    (2380, EvidenceStatus::Implemented, "add_nodes_impl (node_manager/memory/memory_mgr_impl.rs:142); opt-in via clients_can_modify_address_space; tested node_management.rs."),
    (2381, EvidenceStatus::Implemented, "agg_maximum engine.rs:888 (2347); test test_calculate_aggregate_min_max aggregates_tests.rs:228"),
    (2382, EvidenceStatus::Implemented, "engine.rs dispatch AGG_MINIMUM2=11286 (engine.rs:1482); test aggregates_tests.rs:580 phase_d_minimum2_includes_simple_bound"),
    (2383, EvidenceStatus::Implemented, "data_history.rs:228-238 update_data PerformUpdateType::Insert; test hda.rs:320-351 e2e_inmemory_update_then_read_roundtrip"),
    (2384, EvidenceStatus::Implemented, "engine.rs dispatch AGG_WORST_QUALITY2=11292 (engine.rs:1485); test aggregates_tests.rs:1141 worst_quality_is_value_type_independent"),
    (2389, EvidenceStatus::Implemented, "Write handler async-opcua-server/src/session/message_handler.rs:820-852; tests async-opcua/tests/integration/write.rs"),
    (2390, EvidenceStatus::Gap, "NonExclusiveDeviationAlarmType only structural (node_ids.rs:10593); no server instantiation"),
    (2391, EvidenceStatus::Implemented, "Call service handled in session/message_handler.rs:411; tested call_trivial/call_args in async-opcua/tests/integration/methods.rs:26,61."),
    (2394, EvidenceStatus::Implemented, "delete_nodes_impl (memory_mgr_impl.rs:329); tested tests/integration/node_management.rs."),
    (2399, EvidenceStatus::Gap, "Searched 'ComplexNumberType' - zero hits outside generated nodeset."),
    (2400, EvidenceStatus::Implemented, "ActivateSession identity-change + revalidate_monitored_items_for_user manager.rs:1565,1591-1598; test manager.rs:2234-2253"),
    (2407, EvidenceStatus::Implemented, "builder.rs: add_user_token:567, SecurityPolicy::None/Sign/SignAndEncrypt:140-195, trust_client_certs:397-398, pki_dir:494; tested security_tests.rs."),
    (2408, EvidenceStatus::Implemented, "agg_worst_quality2 engine.rs:1266 (11292); test worst_quality_is_value_type_independent aggregates_tests.rs:1153-1156"),
    (2422, EvidenceStatus::Partial, "Audit events ride negotiated SecureChannel (Sign/SignAndEncrypt supported) but nothing specifically enforces/verifies encrypted delivery"),
    (2423, EvidenceStatus::Implemented, "RationalNumberType present schemas/1.05/Opc.Ua.NodeSet2.xml, generated types/rational_number.rs; exposed via CoreNamespace import."),
    (2426, EvidenceStatus::Gap, "Searched 'DiscreteItemType' - zero instance usage in server/samples/tests."),
    (2446, EvidenceStatus::Implemented, "HasAddIn ReferenceType via generated core nodeset nodeset_19.rs:822, loaded by default address_space/mod.rs:11"),
    (2447, EvidenceStatus::Implemented, "DefaultInstanceBrowseName Property via generated nodeset_21.rs:2832, loaded by default node_manager/memory/core.rs:172"),
    (2454, EvidenceStatus::Partial, "Call passes arbitrary Vec<Variant> incl ExtensionObject generically (node_manager/method.rs) but no test uses a Structure argument."),
    (2474, EvidenceStatus::Gap, "Searched 'MultiStateDictionaryEntryDiscreteBaseType' - zero hits outside generated type def."),
    (2476, EvidenceStatus::Partial, "Real computed LocalTime (chrono->TimeZoneDataType) node_manager/memory/core.rs:989-997; no test reads Server_LocalTime attribute"),
    (2478, EvidenceStatus::Implemented, "OsClockSource default TimeSyncSource impl async-opcua-server/src/time_sync.rs:112-124; unit test time_sync.rs:130-137"),
    (2479, EvidenceStatus::Partial, "TimeSyncSource extension-point trait exists (time_sync.rs:36-46,81-102); PTP explicitly \"not implemented in-library\" per time_sync.rs:10-17"),
    (2480, EvidenceStatus::Partial, "TimeSyncMechanism::Gptp extension point only (time_sync.rs:39-46); not implemented in-library, same pattern as PTP"),
    (2481, EvidenceStatus::Implemented, "NormalizedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import (core.rs:147)."),
    (2482, EvidenceStatus::Implemented, "DecimalString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2483, EvidenceStatus::Implemented, "DurationString/TimeString/DateString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2484, EvidenceStatus::Implemented, "BitFieldMaskDataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2485, EvidenceStatus::Implemented, "KeyValuePair in nodeset + generated types/key_value_pair.rs; used by published_data_set_data_type.rs."),
    (2486, EvidenceStatus::Implemented, "core.rs:838-843 Server_ServerCapabilities_MaxHistoryContinuationPoints wired to limits.max_history_continuation_points; consumed by continuation.rs cache"),
    (2487, EvidenceStatus::Implemented, "core.rs:882-887 MaxNodesPerHistoryUpdateEvents wired to limits.operational.max_nodes_per_history_update"),
    (2488, EvidenceStatus::Implemented, "core.rs:876-881 MaxNodesPerHistoryUpdateData wired to limits.operational.max_nodes_per_history_update"),
    (2489, EvidenceStatus::Implemented, "MaxNodesPerNodeManagement live-wired (node_manager/memory/core.rs:894-898), node-management feature."),
    (2490, EvidenceStatus::Implemented, "HasStructuredComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2491, EvidenceStatus::Implemented, "AssociatedWith present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2500, EvidenceStatus::Implemented, "EUInformation used/tested: tests/integration/custom_types.rs, async-opcua-types/src/tests/json.rs:344."),
    (2512, EvidenceStatus::Gap, "Searched 'OrderedListType'/'IOrderedObjectType' - zero instance usage anywhere."),
    (2513, EvidenceStatus::Implemented, "AudioVariableType/AudioDataType present schemas/1.05/Opc.Ua.NodeSet2.xml; type-level exposure via CoreNamespace import."),
    (2514, EvidenceStatus::Implemented, "VectorType/CartesianCoordinatesType/OrientationType/FrameType present in schemas/1.05; exposed via CoreNamespace import."),
    (2515, EvidenceStatus::Implemented, "Server EventNotifier=1 (nodeset_16.rs:989); BaseEventType/GeneratesEvent/EventTypes in nodeset; used in subscriptions.rs tests"),
    (2516, EvidenceStatus::Implemented, "HasOrderedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2517, EvidenceStatus::Implemented, "IsDeprecated present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2518, EvidenceStatus::Implemented, "ImageBMP/GIF/JPG/PNG present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (2526, EvidenceStatus::Implemented, "core.rs:864-869 MaxNodesPerHistoryReadData wired; enforced+tested in async-opcua/tests/integration/read.rs:1183-1208 (BadTooManyOperations)"),
    (2527, EvidenceStatus::Implemented, "core.rs:870-875 MaxNodesPerHistoryReadEvents wired to limits.operational.max_nodes_per_history_read_events; enforced in session/services/attribute.rs:126-134"),
    (2536, EvidenceStatus::Implemented, "ContentFilter/Element DataTypes+encodings (node_ids.rs:168,7132); real WhereClause use+tests in where_clause.rs:13-56, select.rs:14-79."),
    (2539, EvidenceStatus::Gap, "Searched 'HasDictionaryEntry' - only a type node in generated nodeset; no server code links dictionary entries."),
    (2600, EvidenceStatus::Implemented, "10+ SecurityPolicy variants incl None async-opcua-crypto/src/security_policy.rs:125-150; extensively tested + CI conformance matrix"),
    (2649, EvidenceStatus::Gap, "Searched 'GuardVariable'/'ChoiceState' - zero hits repo-wide."),
    (2664, EvidenceStatus::Gap, "no \"modified\" tracking exists for the annotation_values store (data_history.rs record_modified is only invoked from update_data, never update_structure_data at lines 290-355); ReadModified=true has no structured-data source"),
    (2705, EvidenceStatus::Gap, "No \"Azure\" match in src; no authorityProfileURI concept exists anywhere (grep -r confirmed)."),
    (2709, EvidenceStatus::Gap, "No Part-12 GetAccessToken Methods/AuthorizationServices instantiated; only unused generated defs (nodeset_18.rs, node_ids.rs)."),
    (2711, EvidenceStatus::Gap, "Only generated node IDs (SelectionListType_RestrictToList/Selections node_ids.rs:13907,15279); zero application code/tests"),
    (2726, EvidenceStatus::Gap, "FirstInGroup only in generated nodeset (node_ids.rs, nodeset_23/8.rs); zero server code in async-opcua-server/src"),
    (2730, EvidenceStatus::Implemented, "engine.rs dispatch AGG_RANGE2=11288 (engine.rs:1484); aggregates_tests.rs references 11288 (phase_d family)"),
    (2740, EvidenceStatus::Gap, "delete_raw_modified/delete_at_time (data_history.rs:357-466) only operate on raw_values/modified_values, never annotation_values; structured-data removal is only reachable via UpdateStructureDataDetails(Remove), itself restricted to Annotation-typed values (line 305), so still fails the generic-structured-data bar"),
    (2743, EvidenceStatus::Gap, "AggregateFunction_End=2358 (node_ids.rs:7365) NOT in SUPPORTED_AGGREGATE_IDS engine.rs:44-79; only EndBound(11506) implemented"),
    (2746, EvidenceStatus::Implemented, "LimitAlarmKind::Level (limit.rs) parameterizes create_exclusive_in_address_space to report ExclusiveLevelAlarmType via register_level_alarm; test alarms.rs::level_alarm_reports_level_type_definition_not_generic_limit_and_activates"),
    (2747, EvidenceStatus::Gap, "Only structural ObjectType (nodeset_19.rs); grep of async-opcua-server/src for SystemStatusChangeEventType shutdown-event emission: empty"),
    (2754, EvidenceStatus::Implemented, "agg_interpolative engine.rs:1344 (2341); tests aggregates_tests.rs:505,534,927"),
    (2759, EvidenceStatus::Implemented, "engine.rs dispatch AGG_MINIMUM_ACTUAL_TIME=2348 (engine.rs:1478); test aggregates_tests.rs:345 phase_b_actual_time_returns_value_timestamp_not_interval_start"),
    (2772, EvidenceStatus::Gap, "Searched 'SemanticChange' - no StatusCode info-bit constant exists anywhere in async-opcua-types."),
    (2776, EvidenceStatus::Gap, "Depends on MultiStateDictionaryEntryDiscreteBaseType (gap); 'ValueAsDictionaryEntries' not found."),
    (2781, EvidenceStatus::Implemented, "is_writable() enforces WriteMask per attribute (utils.rs:64-128); tests utils.rs:917-1017 + write.rs:538 (BadNotWritable)."),
    (2785, EvidenceStatus::Implemented, "ServerBuilder host()/port()/endpoint config: async-opcua-server/src/builder.rs:531,543,548."),
    (2786, EvidenceStatus::Partial, "TimeSyncMechanism::Ntp extension point only (time_sync.rs:33-35); explicitly \"not implemented in-library\" per doc comment"),
    (2802, EvidenceStatus::Implemented, "AddRole/RemoveRole (role_management.rs:375-469), SecurityAdmin-gated; unit test :627; e2e rbac.rs:706,921 (wire-level pass+deny)."),
    (2806, EvidenceStatus::Gap, "No runtime Write path sets RolePermissions: SimpleNodeManager::write rejects non-Value attrs (simple.rs:1178); only set at node-creation."),
    (2808, EvidenceStatus::Implemented, "Opt-in RBAC enforcement async-opcua-server/src/rbac/decision.rs:46-81; dedicated suite async-opcua/tests/integration/rbac.rs"),
    (2809, EvidenceStatus::Implemented, "AccessLevelExType NonatomicRead/Write async-opcua-nodes/src/variable.rs:62,827-837; unit test variable.rs:990-997"),
    (2811, EvidenceStatus::Partial, "ProgramStateMachine (programs/state.rs) + ShelvingStateMachine (alarms/state_machine.rs) real+tested, but no GeneratesEvent wiring found."),
    (2813, EvidenceStatus::Gap, "Searched 'AvailableStates'/'AvailableTransitions' - zero hits outside generated type def."),
    (2814, EvidenceStatus::Partial, "ProgramStateMachine/ShelvingStateMachine real instances w/ tests, but AvailableStates/AvailableTransitions not populated."),
    (2817, EvidenceStatus::Gap, "UserTokenPolicy.issuer_endpoint_url hardcoded UAString::null() (authenticator.rs:327,341,353; session/manager.rs:2742) — never set."),
    (2818, EvidenceStatus::Partial, "Monitored-item sampling reuses Read's Variant pipeline (subscriptions/mod.rs:1230) but no test monitors a structured value."),
    (2820, EvidenceStatus::Partial, "WriteFullArrayOnly bit stored/read/written async-opcua-nodes/src/variable.rs:62,831 but never enforced against IndexRange array writes"),
    (2822, EvidenceStatus::Gap, "DeviceFailureEventType only structural (nodeset_19.rs); no server code constructs/fires it (grep across async-opcua-server/src empty)"),
    (2823, EvidenceStatus::Partial, "Fixed 100ms tarpit on every auth failure (session/negotiate.rs:16,28-40; tested security_tests.rs:2429); no escalating lockout."),
    (2831, EvidenceStatus::Gap, "Searched 'MultiStateValueDiscreteType' - zero instance usage in server/samples/tests."),
    (2837, EvidenceStatus::Implemented, "BinaryEncodable/BinaryDecodable traits async-opcua-types/src/encoding.rs:445-482, pervasive derive use; tests encoding.rs:919"),
    (2845, EvidenceStatus::Gap, "Only generated NodeId constants (ServerType_RequestServerStateChange, node_ids.rs:1103-1105); no add_method_cb handler found anywhere."),
    (2852, EvidenceStatus::Gap, "condition_sub_class_id field exists on BaseEventType only (events/event.rs:70-73); never set anywhere in async-opcua-server/src"),
    (2853, EvidenceStatus::Implemented, "SecureChannel/OpenSecureChannel comms/secure_channel.rs:657; tests secure_channel.rs:136-663, integration secure_channel.rs:15"),
    (2861, EvidenceStatus::Gap, "DiscrepancyAlarmType only structural (node_ids.rs:10659, generated/events.rs:479); no server instantiation"),
    (2863, EvidenceStatus::Implemented, "Modern policies default-on, legacy Basic128Rsa15/Basic256 opt-in behind legacy-crypto feature builder.rs:142-166; matrix test"),
    (2867, EvidenceStatus::Implemented, "async-opcua-server/src/reverse_connect.rs (ReverseConnectionManager etc.); e2e async-opcua/tests/integration/reverse_connect.rs:16-17 test_reverse_connect."),
    (2871, EvidenceStatus::Gap, "GetEndpoints handler (session/controller.rs:697-707) has no Transport-URI \"SL\" query-string filter/sessionless-endpoint logic."),
    (2873, EvidenceStatus::Gap, "DefaultRolePermissions only settable via ServerBuilder config pre-startup (builder.rs:439); no live Write path (core.rs:1104)."),
    (2877, EvidenceStatus::Gap, "OnDelay/OffDelay only in generated nodeset; zero hits in async-opcua-server/src or async-opcua-client/src"),
    (2879, EvidenceStatus::Gap, "ReAlarmTime/ReAlarmRepeatCount only in generated nodeset; zero server implementation"),
    (2881, EvidenceStatus::Gap, "AudibleSound only in generated nodeset; zero server implementation"),
    (2893, EvidenceStatus::Implemented, "AlarmConditionType_Suppress/Unsuppress Methods registered (methods.rs); handle_condition_suppress/unsuppress call set_suppressed; test alarms.rs::suppress_unsuppress_methods_toggle_suppressed_state"),
    (2896, EvidenceStatus::Implemented, "SilenceState variable added (state_machine.rs) + AlarmConditionType_Silence Method registered; handle_condition_silence calls set_silenced; test alarms.rs::silence_method_toggles_silence_state_and_is_idempotent"),
    (2897, EvidenceStatus::Implemented, "SuppressedState var+get/set_suppressed wired to SuppressedOrShelved (state_machine.rs), now tested via alarms.rs::suppress_unsuppress_methods_toggle_suppressed_state"),
    (2902, EvidenceStatus::Gap, "Server validates OAuth2 JWTs (crypto/identity/jwt_validator.rs) but no HTTPS token-fetch flow to an OAuth2 authority exists."),
    (2918, EvidenceStatus::Partial, "ObjectBuilder::has_event_source exists (async-opcua-nodes/src/object.rs:49-56) but zero call sites building a hierarchy; alarms wire HasCondition only (alarms/limit.rs:351), not HasEventSource."),
    (2921, EvidenceStatus::Implemented, "Active/Acked/Confirmed/Retain/Severity/Message/branch mechanics (state_machine.rs, transitions.rs); test alarms.rs:64"),
    (2927, EvidenceStatus::Implemented, "handle_ack_method methods.rs:65-150 + AcknowledgeableConditionType_Acknowledge registered methods.rs:654-658; tested alarms.rs:64,706"),
    (2928, EvidenceStatus::Implemented, "Absolute DataChangeFilter deadband subscriptions/monitored_item/filters.rs:128-137; unit test filters.rs:175"),
    (2929, EvidenceStatus::Implemented, "data_history.rs:79-120 read_modified_values + record_modified (raw data); tests history_data_inmemory.rs:285 replace_is_readable_as_modified_replace, :303 deletes_are_readable_as_modified_delete, :331 never_modified_value_has_no_modified_entry"),
    (2936, EvidenceStatus::Partial, "Write stores client status/source_timestamp async-opcua-nodes/src/variable.rs:737-739; no test reads value back post-Write"),
    (2937, EvidenceStatus::Gap, "update_structure_data (data_history.rs:290-355) rejects non-Annotation values at line 306 (BadTypeMismatch); same restriction in sqlite backend.rs:1030 — no generic structured-data update"),
    (2939, EvidenceStatus::Implemented, "add_references_impl (memory_mgr_impl.rs:414); tested memory_mgr_impl.rs:2453 (mismatch rejection)."),
    (2940, EvidenceStatus::Implemented, "GetMonitoredItems method node_manager/memory/core.rs:1195-1207; test methods.rs:291-332 call_get_monitored_items"),
    (2941, EvidenceStatus::Implemented, "agg_maximum_actual_time2 engine.rs:950 (11306); test aggregates_tests.rs:952 (duplicate-extrema)"),
    (2943, EvidenceStatus::Implemented, "event_history.rs:226-250 delete_event; tests history_events_inmemory.rs:128 delete_event_by_id + sqlite history_events.rs:117"),
    (2946, EvidenceStatus::Gap, "NonExclusiveRateOfChangeAlarmType only structural (node_ids.rs:10592); no server instantiation"),
    (2947, EvidenceStatus::Implemented, "event_history.rs:68-138 read_events using ParsedEventFilter; test history_tests.rs:407 test_history_read_events_empty_result"),
    (2948, EvidenceStatus::Implemented, "engine.rs dispatch AGG_VARIANCE_POPULATION=11429 (engine.rs:1520); test aggregates_tests.rs:369 phase_b_variance_and_stddev"),
    (2950, EvidenceStatus::Partial, "both backends persist a distinct server_timestamp (sqlite backend.rs:105/417/854 dedicated column, query.rs:93 populates on read; in-memory stores full DataValue); config flag capabilities.rs:34 defaults false; no test asserts server_timestamp survives distinct from source_timestamp on read, and simple.rs history_read_raw_modified ignores timestamps_to_return (unused param)"),
    (2951, EvidenceStatus::Gap, "ExclusiveDeviationAlarmType only structural (node_ids.rs:10589); no server instantiation"),
    (2952, EvidenceStatus::Implemented, "agg_minimum_actual_time2 engine.rs:863 (11305); test phase_d_min_actual_time2_uses_bound_timestamp aggregates_tests.rs:603"),
    (2954, EvidenceStatus::Implemented, "agg_duration_bad engine.rs:1162 (2361); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694"),
    (2955, EvidenceStatus::Implemented, "agg_std_dev_population engine.rs:1033 (11427); test phase_b_variance_and_stddev aggregates_tests.rs:369-393"),
    (2957, EvidenceStatus::Implemented, "handle_condition_refresh methods.rs:359-367; tested alarms.rs:511 condition_refresh_delivers_retained_alarm_to_late_subscriber"),
    (2958, EvidenceStatus::Implemented, "agg_count engine.rs:1067 (2352); tests aggregates_tests.rs:320, count_boolean_source_counts:1003"),
    (2960, EvidenceStatus::Implemented, "engine.rs dispatch AGG_VARIANCE_SAMPLE=11428 (engine.rs:1519); test aggregates_tests.rs:369 phase_b_variance_and_stddev"),
    (2962, EvidenceStatus::Implemented, "engine.rs dispatch AGG_MAXIMUM=2347 (engine.rs:1477); test aggregates_tests.rs:228 test_calculate_aggregate_min_max"),
    (2963, EvidenceStatus::Implemented, "create/modify/delete_monitored_items + set_monitoring_mode (session/services/monitored_items.rs:170-573); tested subscriptions.rs."),
    (2965, EvidenceStatus::Implemented, "ConditionStateMachine base creates EnabledState/Retain/etc for every condition (state_machine.rs:126-256); foundational to all A&C tests"),
    (2969, EvidenceStatus::Gap, "ValueAsText only a static generated nodeset property (nodeset_20.rs:3350); no server code computes/updates it from enum values"),
    (2974, EvidenceStatus::Implemented, "agg_minimum_actual_time engine.rs:826 (2348); test phase_b_actual_time_returns_value_timestamp aggregates_tests.rs:345"),
    (2975, EvidenceStatus::Implemented, "engine.rs dispatch AGG_PERCENT_BAD=2363 (engine.rs:1502); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad"),
    (2978, EvidenceStatus::Gap, "SemanticChangeEventType only a generated type (events/generated.rs:699); never raised; no semantic-changed StatusCode bit usage"),
    (2984, EvidenceStatus::Gap, "Searched 'DoubleComplexNumberType' - zero hits outside generated nodeset."),
    (2985, EvidenceStatus::Implemented, "engine.rs dispatch AGG_NUMBER_OF_TRANSITIONS=2355 (engine.rs:1495-1497); tests aggregates_tests.rs:1060 transitions_boolean_counts_each_flip, :1076 transitions_value_change_not_zero_crossing"),
    (2988, EvidenceStatus::Gap, "Searched 'MultiStateDiscreteType' - zero instance usage in server/samples/tests."),
    (2991, EvidenceStatus::Gap, "depends on ReadAtTimeDetails, which has zero server-side implementation for any backend (see CU 3020); a fortiori unsupported for structured data"),
    (2993, EvidenceStatus::Implemented, "engine.rs dispatch AGG_ANNOTATION_COUNT=2351 (engine.rs:1493); test aggregates_tests.rs:1275 annotation_count_counts_annotations_in_interval; cross-backend parity via history_data_inmemory.rs:441 + sqlite history_update_data.rs:455"),
    (2996, EvidenceStatus::Implemented, "engine.rs dispatch AGG_AVERAGE=2342 (engine.rs:1473); test aggregates_tests.rs:203 test_calculate_aggregate_average"),
    (2998, EvidenceStatus::Implemented, "agg_duration_in_state_zero engine.rs:1197 (11307); test duration_in_state_boolean_splits aggregates_tests.rs:1169"),
    (3000, EvidenceStatus::Implemented, "docs/setup.md gives install/toolchain/feature-flag/cert-loading instructions."),
    (3001, EvidenceStatus::Implemented, "LimitAlarmKind::Level (limit.rs) parameterizes create_non_exclusive_in_address_space to report NonExclusiveLevelAlarmType via register_level_alarm; same evaluation path as CU 2746, no dedicated non-exclusive test yet"),
    (3004, EvidenceStatus::Implemented, "discrete.rs covers OffNormalAlarmType+TripAlarmType, both DiscreteAlarmType subtypes (discrete.rs:1-2,182-186); tested alarms.rs:1176,2421"),
    (3006, EvidenceStatus::Implemented, "engine.rs dispatch AGG_STANDARD_DEVIATION_SAMPLE=11426 (engine.rs:1513-1515); test aggregates_tests.rs:173 test_calculate_std_dev_sample, :369 phase_b_variance_and_stddev"),
    (3010, EvidenceStatus::Implemented, "agg_percent_bad engine.rs:1183 (2363); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694"),
    (3011, EvidenceStatus::Implemented, "engine.rs dispatch AGG_RANGE=2350 (engine.rs:1480); test aggregates_tests.rs:320 phase_b_count_average_range_delta"),
    (3015, EvidenceStatus::Gap, "update_structure_data Replace arm (data_history.rs:322-331) gated by is_annotation_data_value (line 305) — same annotation-only restriction, no generic structured data replace"),
    (3018, EvidenceStatus::Implemented, "engine.rs dispatch AGG_MAXIMUM_ACTUAL_TIME=2349 (engine.rs:1479); test aggregates_tests.rs:345 phase_b_actual_time_returns_value_timestamp_not_interval_start"),
    (3020, EvidenceStatus::Gap, "node_manager/mod.rs:433 declares history_read_at_time (default BadHistoryOperationUnsupported at memory_mgr_impl.rs:1759-1767); simple.rs has no override for it (raw_modified/processed/events/annotations all are overridden there, at_time is not) — ReadAtTimeDetails always fails server-side"),
    (3026, EvidenceStatus::Gap, "Same as 2163: UserWriteMask never varies by user/role anywhere (grep confirms zero dynamic computation); no multilevel test."),
    (3027, EvidenceStatus::Gap, "Same search as Redundancy Server (2258); no transparent-redundancy failover code found."),
    (3032, EvidenceStatus::Implemented, "engine.rs dispatch AGG_TOTAL=2344 (engine.rs:1475); aggregates_tests.rs references 2344 in phase tests"),
    (3043, EvidenceStatus::Gap, "no helper analogous to attach_annotations_property instantiates a per-Variable HistoricalConfiguration+AggregateConfigurationType Object (searched async-opcua-server/src, no hits); middleware.rs read_processed_aggregates (lines 57-106) sources AggregateConfiguration only from the request parameter, never from an address-space node"),
    (3047, EvidenceStatus::Implemented, "agg_range2 engine.rs:757 (11288); test aggregates_tests.rs:580-598 (Range2=15)"),
    (3048, EvidenceStatus::Implemented, "agg_percent_good engine.rs:1169 (2362); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694"),
    (3049, EvidenceStatus::Implemented, "handle_confirm_method methods.rs:225-278 + AcknowledgeableConditionType_Confirm registered methods.rs:668-671; tested alarms.rs:706,268"),
    (3053, EvidenceStatus::Implemented, "data_history.rs:253-268 update_data PerformUpdateType::Update; test history_data_inmemory.rs:93-105 update_data_matrix_matches_sqlite_semantics"),
    (3055, EvidenceStatus::Implemented, "engine.rs dispatch AGG_WORST_QUALITY=2364 (engine.rs:1503); test aggregates_tests.rs:1141 worst_quality_is_value_type_independent"),
    (3060, EvidenceStatus::Gap, "Searched docs/ for locale variants (*.fr.md etc.) and translated dirs — none; all docs English-only."),
    (3061, EvidenceStatus::Gap, "AggregateFunction_End (i=2358 per schemas/1.05/NodeIds.csv:992) is absent from engine.rs AGG_ constants and dispatch (searched, no hit); only the distinct EndBound(11506) aggregate is implemented"),
    (3062, EvidenceStatus::Implemented, "agg_total engine.rs:729 (2344); test phase_f_time_average_excludes_bad_regions aggregates_tests.rs:781-825"),
    (3064, EvidenceStatus::Gap, "No has_notifier reference-builder method exists (only event_notifier attribute setter, object.rs:29); HasNotifier only appears in ref-type-hierarchy declaration (address_space/mod.rs:939), no instance hierarchy built/tested."),
    (3072, EvidenceStatus::Implemented, "Read applies IndexRange via NumericRange::range_of node_manager/memory/core.rs:1079-1080; tests read.rs:1425,794"),
    (3073, EvidenceStatus::Implemented, "RegisterNodes/UnregisterNodes handler session/services/view.rs:540, memory_mgr_impl.rs:1608; e2e test browse.rs:675"),
    (3075, EvidenceStatus::Implemented, "agg_time_average engine.rs:710 (2343); tests aggregates_tests.rs:781,516"),
    (3080, EvidenceStatus::Implemented, "CertificateStore::create_and_store_application_instance_cert certificate_store.rs:265, default builder.rs:119; test crypto.rs:46"),
    (3081, EvidenceStatus::Implemented, "data_history.rs:357-466 delete_raw_modified/delete_at_time; test hda.rs:391-428 e2e_delete_at_time_via_client"),
    (3083, EvidenceStatus::Implemented, "handle_add_comment_method methods.rs:152-222 + ConditionType_AddComment registered methods.rs:660-663; tested alarms.rs:1574"),
    (3084, EvidenceStatus::Implemented, "docs/server.md, docs/advanced_server.md, docs/advanced_features.md describe server functionality."),
    (3085, EvidenceStatus::Implemented, "engine.rs dispatch AGG_DURATION_GOOD=2360 (engine.rs:1499); test aggregates_tests.rs:694 phase_e_duration_and_percent_good_bad"),
    (3098, EvidenceStatus::Implemented, "discrete.rs DiscreteAlarmKind::OffNormal; tested alarms.rs:1176 offnormal_alarm_activates_off_normal, 2421 auto_fires"),
    (3099, EvidenceStatus::Implemented, "agg_number_of_transitions engine.rs:1205 (2355); tests aggregates_tests.rs:1060,1076"),
    (3101, EvidenceStatus::Implemented, "engine.rs dispatch AGG_MAXIMUM_ACTUAL_TIME2=11306 (engine.rs:1490-1492); test aggregates_tests.rs:612 (MaximumActualTime2 case in phase_d_min_actual_time2_uses_bound_timestamp)"),
    (3105, EvidenceStatus::Implemented, "agg_duration_good engine.rs:1155 (2360); test phase_e_duration_and_percent_good_bad aggregates_tests.rs:694"),
    (3107, EvidenceStatus::Implemented, "docs/opcua-foundation-profile-roadmap.md + docs/ctt-conformance.md document supported profiles and certification-test evidence."),
    (3108, EvidenceStatus::Gap, "AggregateFunction_Start=2357 (node_ids.rs:7364) NOT in SUPPORTED_AGGREGATE_IDS engine.rs:44-79; only StartBound(11505) implemented"),
    (3112, EvidenceStatus::Implemented, "PercentDeadband tested vs EURange AnalogItemType (tests/integration/datachange_overflow.rs:151-245)."),
    (3121, EvidenceStatus::Implemented, "monitored_item.rs ParsedAggregateFilter:101,139; e2e test aggregate_filter_average subscriptions.rs:2276,2384"),
    (3125, EvidenceStatus::Implemented, "X509 user cert validated incl. POP sig (info.rs:1291-1332); tests security_tests.rs:1565-1863 (untrusted/expired/revoked)."),
    (3126, EvidenceStatus::Implemented, "agg_time_average2 engine.rs:714 (11285); test phase_d_time_average2_total2 aggregates_tests.rs:621"),
    (3127, EvidenceStatus::Partial, "Only generated core-nodeset OptionSetType node (nodeset_51.rs id 11487); no server code instantiates/handles it, no test"),
    (3130, EvidenceStatus::Implemented, "agg_maximum_actual_time engine.rs:922 (2349); test phase_b_actual_time_returns_value_timestamp aggregates_tests.rs:345"),
    (3137, EvidenceStatus::Gap, "No custom aggregate extensibility; dispatch_aggregate engine.rs:1466 is a fixed closed match, unknown ids -> BadAggregateNotSupported"),
    (3142, EvidenceStatus::Partial, "sample() passes data_encoding through same pipeline as Read (subscriptions/mod.rs:1230), no monitored-item XML/JSON test found."),
    (3143, EvidenceStatus::Implemented, "enqueue_publish_request pops oldest on overflow, returns BadTooManyPublishRequests (session_subscriptions.rs:767); test :1581."),
    (3144, EvidenceStatus::Implemented, "agg_duration_in_state_non_zero engine.rs:1201 (11308); test duration_in_state_boolean_splits aggregates_tests.rs:1169"),
    (3146, EvidenceStatus::Implemented, "SetTriggering handler message_handler.rs:676, actor.rs:104/392/704; e2e tests triggering.rs:43,160"),
    (3147, EvidenceStatus::Implemented, "Variant::set_range_of variant/mod.rs:1641 via Variable::set_value_range variable.rs:746; test write.rs:688,1008"),
    (3150, EvidenceStatus::Implemented, "Full FilterOperator set incl Like/Between/InList/BitwiseAnd/OfType (async-opcua-nodes/src/events/evaluate.rs); tested event_filter_tests.rs"),
    (3153, EvidenceStatus::Implemented, "delete_references_impl (memory_mgr_impl.rs:704); tested node_management.rs / memory_mgr_impl.rs."),
    (3159, EvidenceStatus::Implemented, "engine.rs dispatch AGG_INTERPOLATIVE=2341 (engine.rs:1472); tests aggregates_tests.rs:505 phase_c_interpolative_at_interval_start, :534 phase_c_interpolative_before_data_is_bad_no_data"),
    (3162, EvidenceStatus::Implemented, "engine.rs dispatch AGG_STANDARD_DEVIATION_POPULATION=11427 (engine.rs:1516-1518); test aggregates_tests.rs:369 phase_b_variance_and_stddev"),
    (3165, EvidenceStatus::Implemented, "one_shot_shelve/timed_shelve/unshelve state_machine.rs:671-707 + methods registered methods.rs:674-693; tested alarms.rs:1255,1343"),
    (3171, EvidenceStatus::Implemented, "mDNS responder discovery/mdns.rs:81 start_responder, wired at server.rs:516,525,827,1162; unit tests mdns.rs:521-673."),
    (3175, EvidenceStatus::Implemented, "CreateSession/ActivateSession/CloseSession session/manager.rs; SecurityMode::None optional cert/nonce manager.rs:283-300; test :47,90"),
    (3182, EvidenceStatus::Gap, "No AuthorizationServiceConfigurationType/AccessToken code found; searched \"AuthorizationService\",\"RequestAccessToken\" - zero hits"),
    (3184, EvidenceStatus::Implemented, "Root/Objects/Server + ServerArray/NamespaceArray/ServiceLevel node_manager/memory/core.rs:986-1063; tests browse.rs:35, read.rs:42-43"),
    (3185, EvidenceStatus::Implemented, "Types/ObjectTypes/DataTypes/VariableTypes/ReferenceTypes folders exposed via default CoreNamespace import (core.rs:147)."),
    (3186, EvidenceStatus::Implemented, "ViewsFolder entry point address_space/mod.rs:774-779; test at same location"),
    (3188, EvidenceStatus::Implemented, "Base built-in types present in schemas/1.05; imported via core.rs:147, exercised by address_space/mod.rs test suite."),
    (3189, EvidenceStatus::Implemented, "ServerType is the root of the default AddressSpace; exercised across suite e.g. tests/integration/browse.rs."),
    (3192, EvidenceStatus::Implemented, "EnabledFlag/ServerDiagnosticsSummary/SubscriptionDiagnosticsArray diagnostics/server.rs, core.rs:501-509; e2e read.rs:1604-1841"),
    (3194, EvidenceStatus::Partial, "MaxSelectClauseParameters/MaxWhereClauseParameters nodes exist (nodeset_28.rs:4158) but Value is DataValue::null(), not live-wired."),
    (3196, EvidenceStatus::Implemented, "CU is conditional on the Server using a fixed set of sampling intervals (OPC-10000-5 SS7.9/SS12.8); this server negotiates a continuously-variable client-requested interval per monitored item (sanitize_sampling_interval, subscriptions/monitored_item.rs:299-311), so the precondition never holds and non-exposure of SamplingIntervalDiagnosticsArray is spec-conformant, not a gap -- documented in docs/server-capacity-limits.md"),
    (3197, EvidenceStatus::Implemented, "RoleSet on ServerCapabilities (role_management.rs:479-481); test rbac.rs:287-316 verifies i=15606 + 8 role nodes."),
    (3198, EvidenceStatus::Gap, "EstimatedReturnTime has zero occurrences outside generated files; no wiring near ServerState::Running server_status.rs:198"),
    (3199, EvidenceStatus::Gap, "SystemStatusChangeEventType has no server-side emission on shutdown; server_status.rs/server_handle.rs never calls notify_event/raise_event"),
    (3203, EvidenceStatus::Implemented, "GeneralModelChangeEvent fired on add/delete_nodes/refs (model_change.rs, memory_mgr_impl.rs:325); e2e test node_management.rs:1437."),
    (3206, EvidenceStatus::Implemented, "monitored_item.rs:1052-1085 notify_event inserts EventQueueOverflowEventType on overflow; tested subscriptions.rs:1697-1779"),
    (3207, EvidenceStatus::Implemented, "OptionSet DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3210, EvidenceStatus::Partial, "fota/file_node.rs:207-229 creates a \"Write\" Method node (metadata only) via insert_method; no add_method_cb wiring found — no functional write semantics."),
    (3211, EvidenceStatus::Gap, "FileDirectoryType only as generated NodeId consts/abstract type (node_ids.rs:10624, nodeset_16.rs); no instance created anywhere in server/samples."),
    (3213, EvidenceStatus::Partial, "Session-bound FileType node structure created + tested fota/file_node.rs:120 & tests/fota_integration.rs:41-80, but Open/Read/Write/Close have no callback logic (no add_method_cb found) — non-functional I/O."),
    (3214, EvidenceStatus::Implemented, "Range in nodeset + generated types/range.rs; used as EURange in datachange_overflow.rs, alarms.rs."),
    (3224, EvidenceStatus::Partial, "Fires for AddNodes/DeleteNodes/AddRef/DeleteRef memory_mgr_impl.rs:324,409,699,878 -> audit_events.rs:24-97; only AddNodes tested"),
    (3226, EvidenceStatus::Gap, "HistoryUpdate handler attribute.rs:286-386 has no audit dispatch; AuditHistoryUpdateEventType only generated, never constructed, no test"),
    (3228, EvidenceStatus::Implemented, "dispatch_write_audit (audit.rs:818, message_handler.rs:899) emits AuditWriteUpdateEventType; e2e write.rs:1063."),
    (3230, EvidenceStatus::Implemented, "dispatch_method_audit (audit.rs:799, method.rs:107) emits AuditUpdateMethodEventType; e2e methods.rs:608."),
    (3323, EvidenceStatus::Gap, "Searched 'YArrayItemType' - zero instance usage, only a nodeset type node."),
    (3324, EvidenceStatus::Gap, "Searched 'XYArrayItemType' - zero instance usage, only a nodeset type node."),
    (3325, EvidenceStatus::Gap, "Searched 'ImageItemType' - zero instance usage, only a nodeset type node."),
    (3326, EvidenceStatus::Gap, "Searched 'CubeItemType' - zero instance usage, only a nodeset type node."),
    (3327, EvidenceStatus::Gap, "Searched 'NDimensionArrayItemType' - zero instance usage, only a nodeset type node."),
    (3328, EvidenceStatus::Implemented, "AxisInformation in schemas/1.05 + generated types/axis_information.rs; type-level exposure via CoreNamespace import."),
    (3524, EvidenceStatus::Gap, "Searched 'IrdiDictionaryEntryType' - only a nodeset type node; no instance/dictionary wiring."),
    (3525, EvidenceStatus::Gap, "Searched 'UriDictionaryEntryType' - only a nodeset type node; no instance/dictionary wiring."),
    (3530, EvidenceStatus::Implemented, "Browse/BrowseNext w/ continuation points view.rs:213; tests browse.rs:252, :757 (Bad_ContinuationPointInvalid)"),
    (3532, EvidenceStatus::Implemented, "queue_size clamp monitored_item.rs:314-336, overflow:1067-1110; test datachange_overflow.rs:33-141 (size=2 discardOldest)"),
    (3534, EvidenceStatus::Implemented, "tests/integration/subscriptions.rs:476-509 creates >=2 subscriptions in one session, asserts BadTooManySubscriptions on next"),
    (3535, EvidenceStatus::Implemented, "RetransmissionQueue (retransmission_queue.rs, sized session_subscriptions.rs:1100) + Republish; test subscriptions.rs:1229"),
    (3536, EvidenceStatus::Implemented, "Username/Password encrypted per policy (negotiate.rs:94-207 decrypt_identity_token_secret); tests negotiate.rs:259-330."),
    (3538, EvidenceStatus::Implemented, "RolePermissions/UserRolePermissions/AccessRestrictions enforced (decision.rs:168-195); nodeset types present; tests rbac.rs:106,146,176."),
    (3539, EvidenceStatus::Partial, "SecurityAdmin perms tested (rbac.rs:991); ConfigureAdmin defined (preset.rs:66-76) but no test asserts its perm bits (only :308)."),
    (3540, EvidenceStatus::Partial, "Anonymous perms tested (rbac.rs:977); AuthenticatedUser granted (resolver.rs:502) but perm bitset (preset.rs:34) never asserted."),
    (3541, EvidenceStatus::Partial, "Operator fully tested (rbac.rs:396-498,986); Observer/Engineer/Supervisor exist (preset.rs:39-64) but only node-existence tested."),
    (3542, EvidenceStatus::Partial, "RoleMappingRuleChangedAuditEventType present in generated nodeset (nodeset_16.rs, i=17641); no code ever raises it, untested."),
    (3544, EvidenceStatus::Partial, "ResendData method core.rs:1209-1220, wired subscription.rs:341-342,757; no test found (searched methods.rs, subscriptions.rs)"),
    (3545, EvidenceStatus::Implemented, "Dynamic per-namespace NamespaceMetaData objects diagnostics/node_manager.rs:583-650; e2e test browse.rs:942-967"),
    (3546, EvidenceStatus::Partial, "BaseEventType.local_time field (events/event.rs:52) read by get_value(128) but never assigned anywhere in async-opcua-server/src"),
    (3547, EvidenceStatus::Implemented, "UABinaryFileDataType + Description types present in schemas/1.05; type-level exposure via CoreNamespace import."),
    (3549, EvidenceStatus::Gap, "Depends on OrderedListType (gap) and NodeVersion Property (gap); both searched, zero server-side hits."),
    (3550, EvidenceStatus::Implemented, "StatusResult in nodeset + generated types/status_result.rs; exposed via CoreNamespace import."),
    (3551, EvidenceStatus::Implemented, "UriString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3554, EvidenceStatus::Implemented, "Core AddressSpace all NodeClasses address_space/mod.rs (1454 LOC, unit tests) + opcua-nodes crate; e2e browse.rs:144-167"),
    (3560, EvidenceStatus::Gap, "Searched 'HasInterface'/'BaseInterfaceType' usage - only nodeset type nodes; no server code creates HasInterface refs."),
    (3562, EvidenceStatus::Gap, "Searched 'HasArgumentDescription' usage - only nodeset ref-type node; MethodBuilder args lack description metadata."),
    (3565, EvidenceStatus::Implemented, "Satisfied via subtype: AnalogItemType (subtype of DataItemType) tested datachange_overflow.rs:173; BadOutOfRange in write_validation.rs:280"),
    (3566, EvidenceStatus::Implemented, "Satisfied via subtype: AnalogItemType instances w/ EURange tested (datachange_overflow.rs:173, alarms.rs:1494)."),
    (3567, EvidenceStatus::Implemented, "AnalogItemType instances w/ EURange, exercised in PercentDeadband + A&C limit tests (datachange_overflow.rs:173, alarms.rs:1509)."),
    (3568, EvidenceStatus::Gap, "Searched 'AnalogUnitType' - zero instance usage, only a nodeset type node."),
    (3569, EvidenceStatus::Gap, "Searched 'AnalogUnitRangeType' - zero instance usage, only a nodeset type node."),
    (3571, EvidenceStatus::Gap, "grep \"AlarmMetrics\" finds zero hits in async-opcua-server/src or async-opcua-client/src (only in interop node_modules)"),
    (3572, EvidenceStatus::Gap, "No OPC-COM/DA/AE wrapper code anywhere (grep for COM/OPC-COM across server+client empty); native Rust stack, no COM interop layer"),
    (3574, EvidenceStatus::Implemented, "backend.rs:85-172 read_processed trait default + middleware.rs:57-106 read_processed_aggregates wiring ReadProcessedDetails end-to-end; test history_tests.rs:341 test_history_read_aggregates (client e2e)"),
    (3576, EvidenceStatus::Implemented, "standard nodeset ships HistoryServerCapabilities/AggregateConfiguration Object (i=11203) with property children, generated at async-opcua-core-namespace/src/generated/nodeset_9.rs:483-499, imported by every server via core.rs:147 import_node_set(&CoreNamespace,...)"),
    (3577, EvidenceStatus::Implemented, "supported_aggregates() engine.rs:85-89 returns 35 ids; ParsedAggregateFilter monitored_item.rs:139; e2e subscriptions.rs:2276"),
    (3581, EvidenceStatus::Gap, "QueryApplications Method not implemented; method.rs:131-135 returns BadServiceUnsupported; no callback registered"),
    (3582, EvidenceStatus::Partial, "Pull model impl GetRejectedList+UpdateCertificate pull_methods.rs:338-360, tested; missing GetTrustList + other TrustList pull ops"),
    (3584, EvidenceStatus::Gap, "Zero non-generated source hits for \"KeyCredential\" anywhere in repo (grep across all *.rs excluding generated)"),
    (3586, EvidenceStatus::Gap, "AuthorizationServiceType not implemented; same search as 3182, zero non-generated hits"),
    (3605, EvidenceStatus::Partial, "MaxNodesPerMethodCall wired node_manager/memory/core.rs:888-892 (const->config->response) but no dedicated test found in tests/*.rs referencing it."),
    (3641, EvidenceStatus::Implemented, "DataTypeId::Argument used building Method args async-opcua-nodes/src/method.rs:92; asserted in address_space/mod.rs:1320."),
    (3642, EvidenceStatus::Gap, "No \"durable\" references in async-opcua-server/src; SetSubscriptionDurable NodeId (12749) is a bare generated node, no callback registered"),
    (3644, EvidenceStatus::Implemented, "SemanticVersionString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3645, EvidenceStatus::Implemented, "SecurityPolicy::None UserTokenPolicy supported (authenticator.rs:397,415); tested authenticator.rs:492-518."),
    (3727, EvidenceStatus::Implemented, "CreateSubscription/Publish/Republish/SetPublishingMode etc implemented (subscriptions/session_subscriptions.rs); tested subscriptions.rs."),
    (3747, EvidenceStatus::Implemented, "IsExecutableOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3748, EvidenceStatus::Implemented, "IsExecutingOn present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3749, EvidenceStatus::Implemented, "Controls present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3750, EvidenceStatus::Implemented, "Utilizes present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3751, EvidenceStatus::Implemented, "Requires present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3752, EvidenceStatus::Implemented, "IsPhysicallyConnectedTo present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3753, EvidenceStatus::Implemented, "RepresentsSameEntityAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3754, EvidenceStatus::Implemented, "RepresentsSameHardwareAs present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3755, EvidenceStatus::Implemented, "RepresentsSameFunctionalityAs present schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3756, EvidenceStatus::Implemented, "IsHostedBy present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3757, EvidenceStatus::Implemented, "HasPhysicalComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3758, EvidenceStatus::Implemented, "HasContainedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3759, EvidenceStatus::Implemented, "HasAttachedComponent present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (3760, EvidenceStatus::Gap, "grep \"SuppressionGroup\" finds zero hits in async-opcua-server/src or async-opcua-client/src"),
    (3761, EvidenceStatus::Gap, "InstrumentDiagnosticAlarmType only structural (node_ids.rs:10685); no server instantiation"),
    (3762, EvidenceStatus::Gap, "SystemDiagnosticAlarmType only structural (node_ids.rs:10686); no server instantiation"),
    (3763, EvidenceStatus::Implemented, "ConditionAuditEvent (methods.rs) emits AuditConditionCommentEventType on AddComment, closing the former methods.rs:201 TODO; test alarms.rs::add_comment_emits_audit_condition_comment_event"),
    (3764, EvidenceStatus::Gap, "no AuditConditionEventType for dialog actions anywhere in async-opcua-server/src"),
    (3765, EvidenceStatus::Gap, "no AuditConditionAcknowledgeEventType anywhere in async-opcua-server/src"),
    (3766, EvidenceStatus::Gap, "no AuditConditionConfirmEventType anywhere in async-opcua-server/src"),
    (3767, EvidenceStatus::Gap, "no AuditConditionShelvingEventType anywhere in async-opcua-server/src"),
    (3768, EvidenceStatus::Gap, "no AuditConditionSuppressionEventType anywhere in async-opcua-server/src (suppression itself also unimplemented, 2897/2893)"),
    (3770, EvidenceStatus::Gap, "no latching implemented at all (see 3774), so no latching audit possible"),
    (3771, EvidenceStatus::Implemented, "AuditConditionOutOfServiceEventType emitted for RemoveFromService/PlaceInService (methods.rs notify_out_of_service_audit_event); test alarms.rs::remove_from_service_place_in_service_emit_audit_condition_out_of_service_event"),
    (3772, EvidenceStatus::Gap, "alarms manual set_suppressed/set_shelved (methods.rs) exist, but no external StateMachine auto-triggers Alarm transitions."),
    (3773, EvidenceStatus::Gap, "set_suppressed is manual Method-driven (alarms/state_machine.rs:568); no linked-StateMachine auto-suppression trigger found."),
    (3774, EvidenceStatus::Gap, "no LatchedState variable and no Reset method anywhere in async-opcua-server/src/alarms (grep confirms)"),
    (3775, EvidenceStatus::Gap, "grep \"AlarmGroup\" finds zero hits in async-opcua-server/src or async-opcua-client/src"),
    (3776, EvidenceStatus::Gap, "grep \"GetGroupMemberships\" finds zero hits in async-opcua-server/src or async-opcua-client/src"),
    (3777, EvidenceStatus::Implemented, "LimitConfig high/high_high/low/low_low + validate (limit.rs:58-195); test alarms.rs:891 limit_alarm_exclusive_drives_bands"),
    (3778, EvidenceStatus::Implemented, "LimitDef.severity per level, severity selection (limit.rs:47-56,740-783); tested alarms.rs:923 (severity 400/700 assertions per band)"),
    (3779, EvidenceStatus::Implemented, "LimitDef.deadband + high/low_exceeded hysteresis (limit.rs:701-715); tested alarms.rs:2040 \"deadband cleared\" assertion"),
    (3786, EvidenceStatus::Gap, "Searched 'ArrayItemType' subtype usage - zero instance usage (only nodeset type nodes)."),
    (3802, EvidenceStatus::Implemented, "ServerConfig::max_acceptable_clock_skew_ns config/server.rs:669,998-1002; tests config/server.rs:375-432"),
    (3808, EvidenceStatus::Implemented, "docs/server-capacity-limits.md enumerates every Limits/SubscriptionLimits/OperationalLimits field with its default and configuration method, cross-checked against config/limits.rs's Default impls and the server_conf_limits_match_struct_field_names test"),
    (3810, EvidenceStatus::Gap, "Searched \"GenerateFileForRead\"/\"TemporaryFileTransferType\" across *.rs (excl. generated) — zero implementation hits."),
    (3811, EvidenceStatus::Gap, "Same search as 3810; no CompletionStateMachine/async read implementation found."),
    (3812, EvidenceStatus::Gap, "Searched \"GenerateFileForWrite\"/\"CloseAndCommit\" — zero implementation hits outside generated NodeId constants."),
    (3813, EvidenceStatus::Gap, "Same search as 3812; no async-write CompletionStateMachine implementation found."),
    (3911, EvidenceStatus::Implemented, "core.rs get_attribute now wires MaxMonitoredItemsPerSubscription/MaxSubscriptionsPerSession to their SubscriptionLimits config fields, and MaxSubscriptions/MaxMonitoredItems (no server-wide cap exists) report spec-valid 0 per OPC-10000-5 SS6.3.2; tests read.rs::server_capabilities_max_monitored_items_per_subscription_and_max_subscriptions_per_session, ::server_capabilities_server_wide_max_subscriptions_and_max_monitored_items_are_zero"),
    (3912, EvidenceStatus::Implemented, "core.rs get_attribute wires MaxSessions to Limits.max_sessions (was the only unwired node in this CU per prior audit); test read.rs::server_capabilities_max_sessions_reports_configured_limit"),
    (3913, EvidenceStatus::Implemented, "max_publish_requests_per_subscription=4 (server/src/lib.rs:227); Publish exercised across tests/integration/subscriptions.rs."),
    (3922, EvidenceStatus::Implemented, "SemanticsChanged bit set monitored_item.rs:1012-1042 via EU-range writes session_subscriptions.rs:1238,1290; tests :1668"),
    (3928, EvidenceStatus::Implemented, "Anonymous gated by endpoint.user_token_ids (authenticator.rs:227-238,322-330); e2e session/manager.rs:2289, rbac.rs:238."),
    (3941, EvidenceStatus::Implemented, "DataTypeDefinition wired via DataTypeBuilder.data_type_definition; e2e-tested by custom_types.rs test_data_type_tree_builder."),
    (3965, EvidenceStatus::Implemented, "user_access_level() computed via RBAC (utils.rs:131-152); 2-role tests utils.rs:1020-1083 show differing AccessLevel per role."),
    (3968, EvidenceStatus::Partial, "audit.rs dispatch_* covers Session/Channel/Cert/Cancel/Write/Method+AddNodes/DeleteNodes; no HistoryUpdate audit despite type existing"),
    (3969, EvidenceStatus::Gap, "Searched 'BaseModelChangeEventType'/'NodeVersion' - zero server-side hits (only GeneralModelChangeEventType is implemented)."),
    (3979, EvidenceStatus::Gap, "Searched 'AuditUpdateStateEventType' - only generated struct exists; no state machine emits it."),
    (3983, EvidenceStatus::Implemented, "result.rs:17-58 filter_diagnostic_info masks diag bits; wired attribute.rs/node_management.rs; test per_op_diagnostics.rs"),
    (3985, EvidenceStatus::Implemented, "controller.rs:396 auth-token check, response.rs:207 requestHandle echo, deadline_queue:971-1016 BadTimeout; e2e read.rs:1400-1408"),
    (3994, EvidenceStatus::Gap, "SessionlessInvokeRequestType/ResponseType exist only as generated types (unused); rbac/decision.rs:147 TODO admits \"sessionless: enforce SessionRequired... not done\"; no dispatch path."),
    (3996, EvidenceStatus::Gap, "Searched 'ReferenceDescriptionVariableType'/'HasReferenceDescription' - only nodeset nodes; unused by server code."),
    (4030, EvidenceStatus::Implemented, "OfType evaluated incl. supertypes (evaluate.rs:211-216, fn of_type:358); arity check validation.rs:345; unit test evaluate.rs:1030-1064."),
    (4052, EvidenceStatus::Implemented, "TrimmedString present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (4053, EvidenceStatus::Implemented, "Locations object (i=31915, nodeset_16.rs:918-943) confirmed reachable via Browse from ObjectsFolder; test browse.rs::locations_object_is_reachable_from_objects_folder"),
    (4054, EvidenceStatus::Implemented, "Handle DataType present in schemas/1.05/Opc.Ua.NodeSet2.xml; exposed via CoreNamespace import."),
    (4055, EvidenceStatus::Implemented, "core.rs get_attribute wires MaxMonitoredItemsQueueSize to SubscriptionLimits.max_monitored_item_queue_size, the same limit already enforced at monitored_item.rs:314; test read.rs::server_capabilities_max_monitored_items_queue_size_reports_configured_limit"),
    (4237, EvidenceStatus::Partial, "NonVolatile/Constant bits defined enums.rs:15-19, generic get/set variable.rs:826-838; tests only exercise other AccessLevelEx bits"),
    (4426, EvidenceStatus::Implemented, "Decimal in nodeset + generated types/decimal_data_type.rs; encoded generically as a Structure DataType."),
    (4427, EvidenceStatus::Gap, "AuditClientEventType only a generated stub (node_ids.rs:10730, events/generated.rs:148); server-as-client code never raises it"),
    (4428, EvidenceStatus::Implemented, "AuditConditionSilenceEventType emitted for Silence (methods.rs handle_condition_silence); test alarms.rs::silence_emits_audit_condition_silence_event"),
    (4463, EvidenceStatus::Implemented, "AlarmConditionType_Suppress2/Unsuppress2 Methods registered (methods.rs), routed through the same handlers as Suppress/Unsuppress with apply_optional_comment; test alarms.rs::suppress2_and_place_in_service2_apply_optional_comment"),
    (4464, EvidenceStatus::Implemented, "AlarmConditionType_RemoveFromService2/PlaceInService2 Methods registered (methods.rs) with apply_optional_comment; test alarms.rs::suppress2_and_place_in_service2_apply_optional_comment"),
    (4465, EvidenceStatus::Gap, "no TimedShelve2/OneShotShelve2/Unshelve2 MethodId anywhere; only non-\"2\" shelve methods exist (methods.rs:674-693)"),
    (4466, EvidenceStatus::Partial, "Respond2 impl dialog.rs:200-209 + methods.rs:319-336,711-714 registered, but 0 test coverage (grep \"Respond2\" in test file: 0 hits)"),
    (4467, EvidenceStatus::Implemented, "OutOfServiceState var+get/set_out_of_service (state_machine.rs) exposed via AlarmConditionType_RemoveFromService/PlaceInService Methods (methods.rs); test alarms.rs::remove_from_service_place_in_service_toggle_out_of_service_state"),
    (4500, EvidenceStatus::Gap, "Searched \"ScheduleType\"/\"CalendarEntryType\"/\"DailyScheduleType\" across all *.rs — no matches; only unrelated Part-10 ProgramState (programs/state.rs) exists."),
    (4501, EvidenceStatus::Gap, "Searched \"CalendarType\"/\"DateRangeType\" — no matches anywhere in codebase."),
    (4502, EvidenceStatus::Gap, "No ScheduleType/AddExceptionScheduleElements/RemoveExceptionScheduleElements methods found (Scheduler types entirely absent)."),
    (4503, EvidenceStatus::Gap, "No CalendarType/AddDateListElements/RemoveDateListElements methods found (Scheduler types entirely absent)."),
    (4505, EvidenceStatus::Gap, "Searched \"UserManagement\" — only unused generated UserManagementType defs (nodeset_18.rs:2083); no instantiated Object/Methods."),
    (4957, EvidenceStatus::Implemented, "Per-endpoint user_token_ids admin-selects enabled token types (authenticator.rs:318-366, builder.rs:567); broad test coverage."),
    (5207, EvidenceStatus::Implemented, "No per-subscription item cap below 2 found (server/src/config/limits.rs); 2+ Double items trivially exercised in subscriptions.rs."),
    (5208, EvidenceStatus::Partial, "IndexRange applied to sample monitored_item.rs:931-940 (Variant::range_of); logic tested via read.rs:794-827, no MonitoredItem-level test"),
    (5213, EvidenceStatus::Implemented, "audit.rs:736 AuditOpenSecureChannelEventType, :763 AuditChannelEventType, :928/:442 Create/ActivateSession; test session_audit.rs:18"),
    (5240, EvidenceStatus::Gap, "Only inert CurrencyUnitType/CurrencyUnit template nodes exist (nodeset_22.rs:908-934); no builder API, usage, or test"),
    (5274, EvidenceStatus::Implemented, "AddIdentity/RemoveIdentity (role_management.rs:330-373), wired for 7 well-known roles; unit tests :682,721,913."),
    (5275, EvidenceStatus::Implemented, "AddEndpoint/RemoveEndpoint role_management.rs:282-328; unit tests role_management.rs:743,895 (filter add/remove + gating)."),
    (5276, EvidenceStatus::Implemented, "AddApplication/RemoveApplication role_management.rs:234-280; unit tests role_management.rs:743,810."),
    (5277, EvidenceStatus::Gap, "TrustedApplication absent from WellKnownRole enum (mod.rs:28-45); rules.rs:155 explicitly rejects it as unsupported."),
    (5292, EvidenceStatus::Gap, "No KeyCredential machinery exists at all (see 3584); generic UserName/Password auth exists but is unrelated to this CU"),
    (5293, EvidenceStatus::Gap, "No KeyCredential authentication-mechanism support of any kind implemented (depends on 3584/5292/5301/5302, all gaps)"),
    (5301, EvidenceStatus::Gap, "AMQP SASL PLAIN KeyCredential profile: no AMQP transport or KeyCredential code found in repo"),
    (5302, EvidenceStatus::Gap, "MQTT UserName KeyCredential profile: no KeyCredential code found (MQTT PubSub transport exists but unrelated)"),
    (5303, EvidenceStatus::Gap, "Push Model for KeyCredential Service: zero KeyCredential code found anywhere (see 3584)"),
    (5505, EvidenceStatus::Implemented, "UaHeaderTimeSyncSource polls ResponseHeader.timestamp (time_sync_ua.rs:52-80), configurable builder.rs:258-262; test time_sync.rs:33"),
    (5510, EvidenceStatus::Implemented, "EnabledState.TransitionTime written by set_enabled (state_machine.rs); test alarms.rs::enabled_state_transition_time_updates_on_enable_disable"),
    (5511, EvidenceStatus::Gap, "EnabledState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5512, EvidenceStatus::Gap, "EnabledState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5513, EvidenceStatus::Implemented, "ActiveState.TransitionTime written by set_active (state_machine.rs); test alarms.rs::active_state_transition_time_and_effective_display_name_update_on_activation"),
    (5514, EvidenceStatus::Implemented, "ActiveState.EffectiveTransitionTime written by recompute_effective_state (state_machine.rs); same test as 5513, plus alarms.rs::shelving_updates_effective_transition_time_without_changing_active_state_transition_time"),
    (5515, EvidenceStatus::Implemented, "ActiveState.EffectiveDisplayName written by recompute_effective_state (state_machine.rs); same tests as 5514"),
    (5516, EvidenceStatus::Implemented, "AckedState.TransitionTime written by set_acked (state_machine.rs); test alarms.rs::acked_and_confirmed_state_transition_time_update_on_acknowledge_confirm"),
    (5517, EvidenceStatus::Gap, "AckedState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5518, EvidenceStatus::Gap, "AckedState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5519, EvidenceStatus::Implemented, "ConfirmedState.TransitionTime written by set_confirmed (state_machine.rs); test alarms.rs::acked_and_confirmed_state_transition_time_update_on_acknowledge_confirm"),
    (5520, EvidenceStatus::Gap, "ConfirmedState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5521, EvidenceStatus::Gap, "ConfirmedState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5522, EvidenceStatus::Implemented, "SuppressedState.TransitionTime written by set_suppressed (state_machine.rs); exercised via alarms.rs::suppress_unsuppress_methods_toggle_suppressed_state (095 US2 Method), no dedicated TransitionTime-value assertion yet"),
    (5523, EvidenceStatus::Gap, "SuppressedState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5524, EvidenceStatus::Gap, "SuppressedState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5525, EvidenceStatus::Implemented, "OutOfServiceState.TransitionTime written by set_out_of_service (state_machine.rs); exercised via alarms.rs::remove_from_service_place_in_service_toggle_out_of_service_state (095 US2 Method), no dedicated TransitionTime-value assertion yet"),
    (5526, EvidenceStatus::Gap, "OutOfServiceState.EffectiveTransitionTime not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5527, EvidenceStatus::Gap, "OutOfServiceState.EffectiveDisplayName not implemented; only ActiveState carries Effective* properties in this design (095 US1 scope)"),
    (5528, EvidenceStatus::Implemented, "SilenceState.TransitionTime written by set_silenced (state_machine.rs, 095 US2); exercised via alarms.rs::silence_method_toggles_silence_state_and_is_idempotent, no dedicated TransitionTime-value assertion yet"),
    (5529, EvidenceStatus::Gap, "no SilenceState variable exists at all; no EffectiveTransitionTime property possible"),
    (5530, EvidenceStatus::Gap, "no SilenceState variable exists at all; no EffectiveDisplayName property possible"),
    (5531, EvidenceStatus::Gap, "no LatchedState variable exists at all (3774 also gap), so no TransitionTime property possible"),
    (5532, EvidenceStatus::Gap, "no LatchedState variable exists at all; no EffectiveTransitionTime property possible"),
    (5533, EvidenceStatus::Gap, "no LatchedState variable exists at all; no EffectiveDisplayName property possible"),
    (5534, EvidenceStatus::Implemented, "HighHighState.TransitionTime written by write_non_exclusive_level, only on actual transition (limit.rs); test alarms.rs::limit_state_transition_time_updates_on_threshold_crossing covers the exclusive variant, non-exclusive covered by same write path"),
    (5535, EvidenceStatus::Implemented, "HighState.TransitionTime written by write_non_exclusive_level (limit.rs)"),
    (5536, EvidenceStatus::Implemented, "LowState.TransitionTime written by write_non_exclusive_level (limit.rs)"),
    (5537, EvidenceStatus::Implemented, "LowLowState.TransitionTime written by write_non_exclusive_level (limit.rs)"),
    (5538, EvidenceStatus::Gap, "zero hits for \"EffectiveTransitionTime\" on HighHighState anywhere in repo src"),
    (5539, EvidenceStatus::Gap, "zero hits for \"EffectiveTransitionTime\" on HighState anywhere in repo src"),
    (5540, EvidenceStatus::Gap, "zero hits for \"EffectiveTransitionTime\" on LowState anywhere in repo src"),
    (5541, EvidenceStatus::Gap, "zero hits for \"EffectiveTransitionTime\" on LowLowState anywhere in repo src"),
    (5542, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on HighHighState anywhere in repo src"),
    (5543, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on HighState anywhere in repo src"),
    (5544, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on LowState anywhere in repo src"),
    (5545, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on LowLowState anywhere in repo src"),
    (5546, EvidenceStatus::Gap, "zero hits for \"TransitionTime\" on DialogState anywhere in dialog.rs or repo src"),
    (5547, EvidenceStatus::Gap, "zero hits for \"EffectiveTransitionTime\" on DialogState anywhere in repo src"),
    (5548, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on DialogState anywhere in repo src"),
    (5549, EvidenceStatus::Implemented, "ShelvingState.CurrentState.TransitionTime (the LastTransition equivalent) written by set_shelving_state (state_machine.rs); test alarms.rs::shelving_updates_effective_transition_time_without_changing_active_state_transition_time"),
    (5550, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ShelvingState machinery (state_machine.rs:589-624 stores only current state)"),
    (5551, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ShelvingState machinery"),
    (5552, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ShelvingState machinery"),
    (5553, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ShelvingState machinery"),
    (5554, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ShelvingState machinery"),
    (5555, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ShelvingState machinery"),
    (5556, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on Unshelved sub-state anywhere in repo src"),
    (5557, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on TimedShelved sub-state anywhere in repo src"),
    (5558, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on OneShotShelved sub-state anywhere in repo src"),
    (5559, EvidenceStatus::Implemented, "LimitState.CurrentState.TransitionTime (the LastTransition equivalent) written by write_exclusive_limit_state, only on actual level change (limit.rs); test alarms.rs::limit_state_transition_time_updates_on_threshold_crossing"),
    (5560, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType (limit.rs has no such fields)"),
    (5561, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType"),
    (5562, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType"),
    (5563, EvidenceStatus::Gap, "no per-transition TransitionTime tracking in ExclusiveLimitStateMachineType"),
    (5564, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on LowLow LimitState sub-state anywhere in limit.rs"),
    (5565, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on Low LimitState sub-state anywhere in limit.rs"),
    (5566, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on High LimitState sub-state anywhere in limit.rs"),
    (5567, EvidenceStatus::Gap, "zero hits for \"EffectiveDisplayName\" on HighHigh LimitState sub-state anywhere in limit.rs"),
    (5578, EvidenceStatus::Gap, "ProgressEventType only a generated struct generated.rs:651, used only as an arbitrary test fixture subscriptions.rs:1739; never raised"),
    // 5592 intentionally has no entry here: it is the known "missing from the
    // normalized CU list" data-quality issue, and its displayed status
    // ("source-issue") always comes from write_cu_table's `None` branch, not
    // from this table. An earlier revision carried a Gap-status row here with
    // a self-referential line-number citation to "the tool logic confirming
    // absence" — that citation went stale the moment this file was
    // refactored, and worse, evidence_note() unconditionally consulted this
    // table regardless of the already-decided SourceIssue status, so the
    // displayed report showed status=source-issue paired with Gap-flavored
    // evidence text. Removed rather than re-cited; the generic SourceIssue
    // evidence text below is accurate and needs no per-CU special case.
    (5791, EvidenceStatus::Gap, "No TemporaryFileTransferType or FileTransferStateMachineType instance/implementation found outside generated NodeId enum constants."),
    (5793, EvidenceStatus::Implemented, "OsClockSource (time_sync.rs:112-124) + UA-based source satisfy facet; docs/time-synchronization.md:9-17; tests time_sync.rs:11-22"),
    (5795, EvidenceStatus::Gap, "No durable-subscription capacity doc found; feature itself absent (CU 3642); only stale CU-COVERAGE.md:962 \"needs-proof\" placeholder"),
    (5796, EvidenceStatus::Implemented, "README.md docs.rs/crates.io badges (README.md:3,5,58) + docs/ folder shipped in repo, accessible from GitHub/docs.rs."),
    (5797, EvidenceStatus::Gap, "Searched docs/ and root *.md for \"troubleshoot\"/\"FAQ\"/\"common issue\" — no troubleshooting or diagnostics guide found."),
    (5801, EvidenceStatus::Partial, "Strong support via full 1.05 nodeset import + codegen custom-nodeset gen (samples/custom-codegen); no e2e completeness test; gaps remain."),
    (5806, EvidenceStatus::Implemented, "data_history.rs:131-180 read_raw_modified; tests history_tests.rs:245 test_history_read_100k_page_reads, :299 test_history_read_reversed_intervals; hda.rs:320 e2e_inmemory_update_then_read_roundtrip"),
    (5807, EvidenceStatus::Implemented, "NonExclusiveLimitAlarmType via create_non_exclusive_in_address_space limit.rs:439-503; tested alarms.rs:971,2400"),
    (5808, EvidenceStatus::Implemented, "ExclusiveLimitAlarmType via create_exclusive_in_address_space limit.rs:357-436; tested alarms.rs:891,1033,1499"),
    (5809, EvidenceStatus::Implemented, "LocalOAuth2Validator does real RS256 sig verify (jwt_validator.rs:117-195); tested security_tests.rs:1894-2540."),
    (5812, EvidenceStatus::Implemented, "history.rs:47-113 HistoryUpdateDetails dispatch (UpdateData/UpdateStructureData/UpdateEvent/Delete*); simple.rs:747-865 history_update; InsertData/ReplaceData/UpdateData/DeleteAtTime all independently tested (see CUs 2383/2264/3053/3081)"),
    (5813, EvidenceStatus::Implemented, "history.rs:26-45 HistoryReadDetails (RawModified/AtTime/Processed/Events/Annotations); attribute.rs:194-229 dispatch; ReadRaw/ReadProcessed/ReadEvents/ReadAnnotations all functional and tested (ReadAtTime is the one unsupported sub-mode, see CU 3020, but the \"at least one\" bar is met many times over)"),
    (5868, EvidenceStatus::Implemented, "PortableQualifiedName/PortableNodeId present schemas/1.05 + generated types/portable_node_id.rs; exposed via CoreNamespace import."),
    (5875, EvidenceStatus::Gap, "'ContinuousOptions'/'DescriptionNodeIdDataType' absent from schemas/1.05 and schemas/1.0.4 nodesets and the whole codebase."),
];

#[cfg(test)]
mod tests {
    use super::{generate_markdown_report, parse_snapshot};

    const FIXTURE: &str = r#"
{
  "canonical_profiles": {
    "nano": {
      "display_name": "Nano Embedded Device 2025 Server Profile",
      "name": "Nano Embedded Device 2025 Server Profile",
      "opc_id": 2266,
      "opc_profile_uri": "http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2025"
    }
  },
  "profiles": [
    {
      "display_name": "Data Access Server Facet",
      "name": "Data Access Server Facet",
      "opc_id": 1505,
      "opc_profile_uri": "http://opcfoundation.org/UA-Profile/Server/DataAccess"
    },
    {
      "display_name": "Nano Embedded Device 2025 Server Profile",
      "name": "Nano Embedded Device 2025 Server Profile",
      "opc_id": 2266,
      "opc_profile_uri": "http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2025"
    }
  ],
  "conformance_units": [
    {"opc_id": 2478, "name": "Time Sync - OS based support"},
    {"opc_id": 2479, "name": "Time Sync - IEEE 1588 (PTP)"},
    {"opc_id": 3912, "name": "Base Info Server Capabilities 2"},
    {"opc_id": 99999, "name": "Fixture-only CU outside the audit table"}
  ],
  "relationships": {
    "included_conformance_units": {
      "2266": [2478, 2479, 3912, 5592, 99999],
      "1505": [2478]
    },
    "included_profiles": {}
  }
}
"#;

    #[test]
    fn report_classifies_cu_implemented_extensible_needsproof_and_source_issues() {
        let snapshot = parse_snapshot(FIXTURE).expect("fixture parses");
        let report = generate_markdown_report(&snapshot).expect("fixture has closure data");

        assert!(report.contains("Nano Embedded Device 2025 Server Profile"));
        // Audited 2026-07-15: OS-based time sync is claimed via the default OsClockSource.
        assert!(report.contains("| 2478 | Time Sync - OS based support | implemented |"));
        // Feature 093: PTP is satisfiable only via a user-supplied TimeSyncSource.
        assert!(report.contains("| 2479 | Time Sync - IEEE 1588 (PTP) | extensible |"));
        // 3912 was incidentally covered by the 2026-07-15 audit (partial: some
        // ServerCapabilities fields wired, MaxSessions is not).
        assert!(report.contains("| 3912 | Base Info Server Capabilities 2 | partial |"));
        // A CU genuinely outside the audit table falls back to needs-proof.
        assert!(
            report.contains("| 99999 | Fixture-only CU outside the audit table | needs-proof |")
        );
        assert!(report.contains("| 5592 | Missing from normalized CU list | source-issue |"));
    }

    #[test]
    fn report_normalizes_canonical_names_to_ascii_markdown() {
        let fixture = FIXTURE.replace(
            "Time Sync - OS based support",
            "Time Sync \u{2013} OS based support",
        );
        let snapshot = parse_snapshot(&fixture).expect("fixture parses");
        let report = generate_markdown_report(&snapshot).expect("fixture has closure data");

        assert!(report.contains("| 2478 | Time Sync - OS based support | implemented |"));
    }

    #[test]
    fn report_includes_additional_facets_not_in_the_four_canonical_profiles() {
        let snapshot = parse_snapshot(FIXTURE).expect("fixture parses");
        let report = generate_markdown_report(&snapshot).expect("fixture has closure data");

        assert!(report.contains("## Additional Server Facets (Summary)"));
        assert!(report.contains("Data Access Server Facet"));
        assert!(report.contains("## Full CU Ledger"));
    }

    #[test]
    fn report_fails_fast_on_snapshot_missing_included_relationship_maps() {
        // A snapshot in the older `transitive_cu_closure`-only shape (or any
        // shape lacking both `included_conformance_units` and
        // `included_profiles`) must be rejected explicitly, not silently
        // produce a report with every closure empty.
        let fixture = r#"
{
  "canonical_profiles": {
    "nano": {
      "display_name": "Nano Embedded Device 2025 Server Profile",
      "name": "Nano Embedded Device 2025 Server Profile",
      "opc_id": 2266,
      "opc_profile_uri": "http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2025"
    }
  },
  "conformance_units": [
    {"opc_id": 2478, "name": "Time Sync - OS based support"}
  ],
  "relationships": {
    "transitive_cu_closure": {
      "2266": [2478]
    }
  }
}
"#;
        let snapshot = parse_snapshot(fixture).expect("fixture parses");
        let result = generate_markdown_report(&snapshot);

        assert!(
            result.is_err(),
            "a snapshot with no included_conformance_units/included_profiles must error, not \
             silently render empty closures"
        );
    }

    #[test]
    fn audit_table_is_sorted_by_cu_id_for_binary_search() {
        let ids: Vec<u32> = super::AUDIT_TABLE.iter().map(|(id, _, _)| *id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "AUDIT_TABLE must stay sorted by CU id");

        // `binary_search_by_key` makes no guarantee about which match it
        // returns when a key appears more than once — a duplicate CU id
        // (e.g. two subsystem audits disagreeing and both surviving into the
        // table by mistake) would make classify_cu/evidence_note's result
        // for that CU unspecified rather than merely wrong.
        let mut deduped = ids.clone();
        deduped.dedup();
        assert_eq!(
            ids, deduped,
            "AUDIT_TABLE must not contain duplicate CU ids"
        );
    }
}
