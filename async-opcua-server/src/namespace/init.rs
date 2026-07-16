//! Server namespace initializer.
//! Provides helper functions to register Alarm Conditions and associate their callbacks in the node manager.

#[cfg(feature = "alarms")]
use crate::address_space::AddressSpace;
#[cfg(feature = "alarms")]
use crate::alarms::{
    read_eurange, ConditionStateMachine, DiscreteAlarm, DiscreteAlarmKind, LimitAlarm,
    LimitAlarmKind, LimitConfig, LimitMode,
};
#[cfg(feature = "alarms")]
use opcua_types::{MethodId, NodeId, ReferenceTypeId, StatusCode, Variant};
#[cfg(feature = "alarms")]
use std::sync::Arc;

/// Registers a new Alarm Condition state machine and exposes the standard Acknowledge/Confirm methods.
#[cfg(feature = "alarms")]
pub fn register_alarm_condition(
    address_space: &Arc<opcua_core::sync::RwLock<AddressSpace>>,
    _node_manager: &crate::node_manager::memory::SimpleNodeManager,
    device: &str,
    alarm_type: &str,
    source_node_id: NodeId,
    condition_name: &str,
) -> ConditionStateMachine {
    // 1. Create the state machine nodes in the Address Space
    let state_machine = {
        let space = opcua_core::trace_write_lock!(address_space);
        ConditionStateMachine::create_in_address_space(
            &space,
            device,
            alarm_type,
            source_node_id,
            condition_name,
        )
    };

    // 2. Expose the standard shared Acknowledge/Confirm method declarations on the condition.
    {
        let space = opcua_core::trace_write_lock!(address_space);
        space.insert_reference(
            &state_machine.condition_id,
            &MethodId::AcknowledgeableConditionType_Acknowledge.into(),
            ReferenceTypeId::HasComponent,
        );
        space.insert_reference(
            &state_machine.condition_id,
            &MethodId::AcknowledgeableConditionType_Confirm.into(),
            ReferenceTypeId::HasComponent,
        );
    }

    state_machine
}

/// Registers a new LimitAlarm condition and exposes the standard Acknowledge/Confirm methods.
#[cfg(feature = "alarms")]
pub fn register_limit_alarm(
    address_space: &Arc<opcua_core::sync::RwLock<AddressSpace>>,
    node_manager: &crate::node_manager::memory::SimpleNodeManager,
    device: &str,
    alarm_name: &str,
    source_node_id: NodeId,
    cfg: LimitConfig,
) -> LimitAlarm {
    register_limit_alarm_kind(
        address_space,
        node_manager,
        device,
        alarm_name,
        source_node_id,
        cfg,
        LimitAlarmKind::Limit,
    )
}

/// Registers a new Level alarm condition (`ExclusiveLevelAlarmType`/`NonExclusiveLevelAlarmType`,
/// OPC-10000-9 §5.8.21.2/.3) and exposes the standard Acknowledge/Confirm methods. Level alarms
/// share `LimitAlarm`'s threshold/deadband evaluation exactly; only the `TypeDefinition` differs
/// from the generic `register_limit_alarm`.
#[cfg(feature = "alarms")]
pub fn register_level_alarm(
    address_space: &Arc<opcua_core::sync::RwLock<AddressSpace>>,
    node_manager: &crate::node_manager::memory::SimpleNodeManager,
    device: &str,
    alarm_name: &str,
    source_node_id: NodeId,
    cfg: LimitConfig,
) -> LimitAlarm {
    register_limit_alarm_kind(
        address_space,
        node_manager,
        device,
        alarm_name,
        source_node_id,
        cfg,
        LimitAlarmKind::Level,
    )
}

#[cfg(feature = "alarms")]
fn register_limit_alarm_kind(
    address_space: &Arc<opcua_core::sync::RwLock<AddressSpace>>,
    _node_manager: &crate::node_manager::memory::SimpleNodeManager,
    device: &str,
    alarm_name: &str,
    source_node_id: NodeId,
    cfg: LimitConfig,
    kind: LimitAlarmKind,
) -> LimitAlarm {
    let alarm = {
        let space = opcua_core::trace_write_lock!(address_space);
        let ns = 2;

        match cfg.mode {
            LimitMode::Exclusive => LimitAlarm::create_exclusive_in_address_space(
                &space,
                ns,
                device,
                alarm_name,
                source_node_id,
                cfg,
                kind,
            ),
            LimitMode::NonExclusive => LimitAlarm::create_non_exclusive_in_address_space(
                &space,
                ns,
                device,
                alarm_name,
                source_node_id,
                cfg,
                kind,
            ),
        }
    };

    {
        let space = opcua_core::trace_write_lock!(address_space);
        space.insert_reference(
            &alarm.condition.condition_id,
            &MethodId::AcknowledgeableConditionType_Acknowledge.into(),
            ReferenceTypeId::HasComponent,
        );
        space.insert_reference(
            &alarm.condition.condition_id,
            &MethodId::AcknowledgeableConditionType_Confirm.into(),
            ReferenceTypeId::HasComponent,
        );
    }

    alarm
}

/// Registers a new LimitAlarm condition after validating limits against the source EURange.
///
/// If the source variable does not expose an AnalogItem EURange property, validation is skipped.
#[cfg(feature = "alarms")]
pub fn register_limit_alarm_checked(
    address_space: &Arc<opcua_core::sync::RwLock<AddressSpace>>,
    node_manager: &crate::node_manager::memory::SimpleNodeManager,
    device: &str,
    alarm_name: &str,
    source_node_id: NodeId,
    cfg: LimitConfig,
) -> Result<LimitAlarm, StatusCode> {
    let eurange = {
        let space = opcua_core::trace_read_lock!(address_space);
        read_eurange(&space, &source_node_id)
    };

    if let Some((low, high)) = eurange {
        cfg.validate_against_eurange(low, high)?;
    }

    Ok(register_limit_alarm(
        address_space,
        node_manager,
        device,
        alarm_name,
        source_node_id,
        cfg,
    ))
}

/// Registers a new DiscreteAlarm condition and exposes the standard Acknowledge/Confirm methods.
#[cfg(feature = "alarms")]
pub fn register_discrete_alarm(
    address_space: &Arc<opcua_core::sync::RwLock<AddressSpace>>,
    _node_manager: &crate::node_manager::memory::SimpleNodeManager,
    device: &str,
    alarm_name: &str,
    source_node_id: NodeId,
    kind: DiscreteAlarmKind,
    normal: Variant,
) -> DiscreteAlarm {
    let alarm = {
        let space = opcua_core::trace_write_lock!(address_space);
        let ns = 2;

        DiscreteAlarm::create_in_address_space(
            &space,
            ns,
            device,
            alarm_name,
            source_node_id,
            kind,
            normal,
        )
    };

    {
        let space = opcua_core::trace_write_lock!(address_space);
        space.insert_reference(
            &alarm.condition.condition_id,
            &MethodId::AcknowledgeableConditionType_Acknowledge.into(),
            ReferenceTypeId::HasComponent,
        );
        space.insert_reference(
            &alarm.condition.condition_id,
            &MethodId::AcknowledgeableConditionType_Confirm.into(),
            ReferenceTypeId::HasComponent,
        );
    }

    alarm
}
