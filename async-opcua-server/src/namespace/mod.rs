//! Server namespace management module.

pub mod init;

#[cfg(feature = "alarms")]
pub use init::{
    register_alarm_condition, register_discrete_alarm, register_level_alarm, register_limit_alarm,
    register_limit_alarm_checked,
};
