use super::{get_registry, sys, ParserFn};

#[derive(Debug, Clone, Copy)]
pub(super) enum EventEffect {
    ListenerStarted,
    ListenerStopped,
    RedirectorStarted,
    RedirectorStopped,
}

impl EventEffect {
    pub(super) fn audit_detail(self) -> &'static str {
        match self {
            Self::ListenerStarted => "c2 listener registered via event bus",
            Self::ListenerStopped => "c2 listener deregistered via event bus",
            Self::RedirectorStarted => "c2 redirector registered via event bus",
            Self::RedirectorStopped => "c2 redirector deregistered via event bus",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum OutputEffect {
    Event(EventEffect),
    DeployContainer,
    SysHasBinary,
    SysHasFile,
    Nmap,
    SelfSubjectRulesReview,
    FileContent,
    Kubeconfig,
    SysNodeName,
    RawServiceAccountToken,
    Registered(ParserFn),
}

impl OutputEffect {
    /// Resolve every output-derived, argument-derived, or event-sourced effect
    /// understood by this module. Structural effects deliberately return None
    /// so the caller can route them to `effects::parse_effect_with_status`.
    pub(super) fn resolve(effect_id: &str) -> Option<Self> {
        let normalized = effect_id.trim().to_ascii_lowercase();
        let effect = if normalized.starts_with("c2.listen(") {
            Self::Event(EventEffect::ListenerStarted)
        } else if normalized.starts_with("c2.stop-listener(") {
            Self::Event(EventEffect::ListenerStopped)
        } else if normalized.starts_with("c2.port-forward(") {
            Self::Event(EventEffect::RedirectorStarted)
        } else if normalized.starts_with("c2.stop-port-forward(") {
            Self::Event(EventEffect::RedirectorStopped)
        } else if normalized == "create k8s.pod"
            || normalized == "namespace($ns)"
            || normalized == "ns.contains($p2)"
            || normalized.starts_with("created(")
        {
            Self::DeployContainer
        } else if normalized.starts_with("sys.has-binary(") {
            Self::SysHasBinary
        } else if normalized.starts_with("sys.hasfile(") {
            Self::SysHasFile
        } else if normalized == "nmap" {
            Self::Nmap
        } else if normalized == "k8s.selfsubjectrulesreview" {
            Self::SelfSubjectRulesReview
        } else if normalized == "file:content" || normalized.starts_with("file:content(") {
            Self::FileContent
        } else if normalized == "file:kubeconfig" {
            Self::Kubeconfig
        } else if normalized == "sys.node-name" {
            Self::SysNodeName
        } else if normalized == "rawserviceaccounttoken" {
            Self::RawServiceAccountToken
        } else {
            Self::Registered(*get_registry().get(normalized.as_str())?)
        };
        Some(effect)
    }

    pub(super) fn stdout_optional(self, effect_id: &str) -> bool {
        match self {
            Self::Event(_) | Self::DeployContainer => true,
            Self::SysHasBinary => {
                let inner = sys::extract_effect_args(effect_id).unwrap_or("");
                !inner.eq_ignore_ascii_case("${output}") && !inner.eq_ignore_ascii_case("output")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_and_special_effects_resolve_through_one_declaration_path() {
        for effect in get_registry().keys() {
            assert!(OutputEffect::resolve(effect).is_some(), "{effect}");
        }

        for effect in [
            "c2.listen(4444, tcp)",
            "create k8s.Pod",
            "sys.has-binary(/usr/bin/curl)",
            "sys.hasFile(/etc/passwd)",
            "nmap",
            "k8s.SelfSubjectRulesReview",
            "file:content(/etc/passwd)",
            "file:kubeconfig",
            "sys.node-name",
        ] {
            assert!(OutputEffect::resolve(effect).is_some(), "{effect}");
        }
        assert!(OutputEffect::resolve("file:local-kubeconfig").is_none());
        assert!(OutputEffect::resolve("unsupported.effect").is_none());
    }
}
