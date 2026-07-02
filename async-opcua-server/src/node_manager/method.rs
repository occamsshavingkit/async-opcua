use opcua_types::{
    CallMethodRequest, CallMethodResult, DiagnosticBits, DiagnosticInfo, NodeId, StatusCode,
    Variant,
};

use super::IntoResult;

#[derive(Debug)]
/// Container for a single method call in a `Call` service call.
pub struct MethodCall {
    object_id: NodeId,
    method_id: NodeId,
    arguments: Vec<Variant>,
    diagnostic_bits: DiagnosticBits,

    status: StatusCode,
    argument_results: Vec<StatusCode>,
    outputs: Vec<Variant>,
    diagnostic_info: Option<DiagnosticInfo>,
}

impl MethodCall {
    pub(crate) fn new(request: CallMethodRequest, diagnostic_bits: DiagnosticBits) -> Self {
        Self {
            object_id: request.object_id,
            method_id: request.method_id,
            arguments: request.input_arguments.unwrap_or_default(),
            status: StatusCode::BadMethodInvalid,
            argument_results: Vec::new(),
            outputs: Vec::new(),
            diagnostic_bits,
            diagnostic_info: None,
        }
    }

    /// Set the argument results to a list of errors.
    /// This will update the `status` to `BadInvalidArgument`.
    ///
    /// The length of `argument_results` must be equal to the length of `arguments`.
    pub fn set_argument_error(&mut self, argument_results: Vec<StatusCode>) {
        self.argument_results = argument_results;
        self.status = StatusCode::BadInvalidArgument;
    }

    /// Set the result of this method call.
    pub fn set_status(&mut self, status: StatusCode) {
        self.status = status;
    }

    /// Set the outputs of this method call.
    pub fn set_outputs(&mut self, outputs: Vec<Variant>) {
        self.outputs = outputs;
    }

    /// Get the arguments to this method call.
    pub fn arguments(&self) -> &[Variant] {
        &self.arguments
    }

    /// Get the ID of the method to call.
    pub fn method_id(&self) -> &NodeId {
        &self.method_id
    }

    /// Get the ID of the object the method is a part of.
    pub fn object_id(&self) -> &NodeId {
        &self.object_id
    }

    /// Get the current status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Header diagnostic bits for requesting operation-level diagnostics.
    pub fn diagnostic_bits(&self) -> DiagnosticBits {
        self.diagnostic_bits
    }

    /// Set diagnostic infos, you don't need to do this if
    /// `diagnostic_bits` are not set.
    pub fn set_diagnostic_info(&mut self, diagnostic_info: DiagnosticInfo) {
        self.diagnostic_info = Some(diagnostic_info);
    }
}

impl IntoResult for MethodCall {
    type Result = CallMethodResult;

    fn into_result(self) -> (Self::Result, Option<DiagnosticInfo>) {
        let input_argument_diagnostic_infos =
            if !self.diagnostic_bits.is_empty() && !self.argument_results.is_empty() {
                Some(vec![DiagnosticInfo::default(); self.argument_results.len()])
            } else {
                None
            };

        (
            CallMethodResult {
                status_code: self.status,
                input_argument_diagnostic_infos,
                input_argument_results: if !self.argument_results.is_empty() {
                    Some(self.argument_results)
                } else {
                    None
                },
                // Part 4 §5.12 (line 3953): outputArguments shall be empty if the statusCode
                // severity is Bad.
                output_arguments: if self.status.is_bad() {
                    None
                } else {
                    Some(self.outputs)
                },
            },
            self.diagnostic_info,
        )
    }
}

/// Convenient macro for performing an _implicit_ cast of
/// each argument to the expected method argument type, and returning
/// the arguments as a tuple.
///
/// This macro will produce `Result<(Arg1, Arg2, ...), StatusCode>`.
///
/// The types in the argument list must be enum variants of the `Variant` type.
///
/// # Example
///
/// ```ignore
/// let (arg1, arg2) = load_method_args!(method_call, Int32, String)?;
/// ```
#[macro_export]
macro_rules! load_method_args {
    ($call:expr, $($type:ident),+) => {
        {
            let mut arguments = $call.arguments().iter();
            (move || {
                Ok($(
                    match arguments.next().map(|v| v.convert(VariantTypeId::Scalar(VariantScalarTypeId::$type))) {
                        Some(Variant::$type(val)) => val,
                        _ => return Err(StatusCode::BadInvalidArgument),
                    }
                ),*)
            })()
        }

    };
}

#[cfg(test)]
mod tests {
    use super::MethodCall;
    use opcua_types::{CallMethodRequest, DiagnosticBits, NodeId, StatusCode, Variant};

    fn call_with_bad_args(bits: DiagnosticBits) -> MethodCall {
        let mut call = MethodCall::new(
            CallMethodRequest {
                object_id: NodeId::new(1, "obj"),
                method_id: NodeId::new(1, "method"),
                input_arguments: Some(vec![Variant::from(1u32), Variant::from("wrong")]),
            },
            bits,
        );
        call.set_argument_error(vec![StatusCode::Good, StatusCode::BadTypeMismatch]);
        call
    }

    /// Part 4 §5.12.2.2: inputArgumentDiagnosticInfos corresponds to the
    /// inputArguments; present only when diagnostics were requested. Content
    /// is not populated (node-manager extension point), only alignment.
    #[test]
    fn input_argument_diagnostic_infos_gated_on_diagnostic_bits() {
        use super::IntoResult;

        // Requested -> aligned with input_argument_results.
        let call = call_with_bad_args(DiagnosticBits::OPERATIONAL_LEVEL_SYMBOLIC_ID);
        let (result, _) = call.into_result();
        let arg_results = result
            .input_argument_results
            .as_ref()
            .expect("argument results set");
        assert_eq!(arg_results.len(), 2);
        let diags = result
            .input_argument_diagnostic_infos
            .as_ref()
            .expect("nested inputArgumentDiagnosticInfos must be present when requested");
        assert_eq!(diags.len(), arg_results.len());

        // Not requested -> absent.
        let call = call_with_bad_args(DiagnosticBits::empty());
        let (result, _) = call.into_result();
        assert!(result.input_argument_results.is_some());
        assert!(result.input_argument_diagnostic_infos.is_none());

        // No argument results -> nothing to align, absent even when requested.
        let call = MethodCall::new(
            CallMethodRequest {
                object_id: NodeId::new(1, "obj"),
                method_id: NodeId::new(1, "method"),
                input_arguments: None,
            },
            DiagnosticBits::OPERATIONAL_LEVEL_SYMBOLIC_ID,
        );
        let (result, _) = call.into_result();
        assert!(result.input_argument_results.is_none());
        assert!(result.input_argument_diagnostic_infos.is_none());
    }
}
