const DISPATCH_287: &[DispatcherEntry] = &[DispatcherEntry {
    command: "plugin-manifest",
    handler: Handler::Sync(run_plugin_manifest_plan),
}];

#[cfg(test)]
mod tests {
    use super::DISPATCH_287;

    #[test]
    fn dispatch_287_owns_plugin_manifest_runtime_seam() {
        assert_eq!(DISPATCH_287.len(), 1);
        assert_eq!(DISPATCH_287[0].command, "plugin-manifest");
    }
}
