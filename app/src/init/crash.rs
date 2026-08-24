use std::sync::Arc;

pub struct CrashHandler(pub Arc<crashes::Client>);

impl gpui::Global for CrashHandler {}