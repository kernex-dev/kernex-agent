//! REPL actions that write to disk, held to the session's context needs.

use colored::Colorize;
use kernex_core::context::ContextNeeds;
use kernex_runtime::Runtime;
use rustyline::DefaultEditor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `/clear`: close the current conversation. Closing writes to memory, so
/// a session without memory (`needs.store` off, `--no-memory`) leaves the
/// store alone; it has stored nothing to clear. Returns whether memory was
/// touched.
pub async fn clear(runtime: &Runtime, needs: &ContextNeeds) -> bool {
    if !needs.store {
        println!(
            "{}",
            "Nothing to clear: this session keeps no memory (--no-memory).\n".dimmed()
        );
        return false;
    }
    crate::commands::close_conversation(runtime).await;
    println!("{}", "Conversation cleared.\n".dimmed());
    true
}

/// Where the REPL keeps its input history: `None` under `--no-memory`, so
/// the session neither loads nor saves the lines typed into it.
pub fn history_path(data_dir: &Path, needs: &ContextNeeds) -> Option<PathBuf> {
    needs.store.then(|| data_dir.join("history.txt"))
}

pub fn create_editor(history_path: &Option<PathBuf>) -> anyhow::Result<DefaultEditor> {
    let mut rl = DefaultEditor::new()?;
    if let Some(path) = history_path {
        let _ = rl.load_history(path);
    }
    Ok(rl)
}

pub async fn save_history(
    editor: &Arc<tokio::sync::Mutex<DefaultEditor>>,
    history: &Option<PathBuf>,
) {
    let Some(history_path) = history else { return };
    if let Some(parent) = history_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = editor.lock().await.save_history(history_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_glue::context_needs;

    async fn runtime(dir: &tempfile::TempDir) -> Runtime {
        kernex_runtime::RuntimeBuilder::new()
            .data_dir(dir.path().to_str().unwrap())
            .project("demo")
            .channel("cli")
            .build()
            .await
            .unwrap()
    }

    async fn rows(runtime: &Runtime) -> (i64, i64, i64, i64) {
        runtime.store.get_memory_stats("user").await.unwrap()
    }

    #[tokio::test]
    async fn clear_without_memory_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(&dir).await;
        let before = rows(&runtime).await;
        assert!(!clear(&runtime, &context_needs(true)).await);
        assert_eq!(rows(&runtime).await, before);
    }

    #[tokio::test]
    async fn clear_with_memory_closes_the_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(&dir).await;
        let request = kernex_core::message::Request::text("user", "hello");
        let response = kernex_core::message::Response {
            text: "hi".to_string(),
            ..Default::default()
        };
        runtime
            .store
            .store_exchange(&runtime.channel, &request, &response, "demo")
            .await
            .unwrap();
        let active = || async { runtime.store.find_all_active_conversations().await.unwrap() };
        assert_eq!(active().await.len(), 1);

        assert!(clear(&runtime, &context_needs(false)).await);
        assert!(
            active().await.is_empty(),
            "the conversation is still active"
        );
    }

    #[tokio::test]
    async fn input_history_is_saved_only_with_memory() {
        let dir = tempfile::tempdir().unwrap();
        let editor = Arc::new(tokio::sync::Mutex::new(DefaultEditor::new().unwrap()));
        editor
            .lock()
            .await
            .add_history_entry("secret line")
            .unwrap();

        let none = history_path(dir.path(), &context_needs(true));
        assert!(none.is_none());
        save_history(&editor, &none).await;
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

        let some = history_path(dir.path(), &context_needs(false));
        save_history(&editor, &some).await;
        assert!(dir.path().join("history.txt").exists());
    }
}
