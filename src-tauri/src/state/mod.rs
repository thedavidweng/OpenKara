pub mod airplay;
pub mod playback;
pub mod remote;
pub mod separation;
pub mod shell;

pub use airplay::AirPlayState;
pub use playback::PlaybackState;
pub use remote::RemoteState;
pub use separation::SeparationState;
pub use shell::AppShell;

use crate::commands::error::CommandError;
use crate::library_root::LibraryRoot;
use crate::lyrics::lrcapi::LrcApiClient;
use crate::lyrics::lrclib::LrcLibClient;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppState {
    pub playback: PlaybackState,
    pub airplay: AirPlayState,
    pub separation: SeparationState,
    pub remote: RemoteState,
    pub shell: AppShell,
    /// Shared HTTP clients for lyrics providers. Cloning a reqwest::Client
    /// shares its internal connection pool, so cloning AppState per command
    /// call reuses TLS connections across song switches instead of paying a
    /// fresh handshake on every fetch.
    pub lrclib_client: LrcLibClient,
    pub lrcapi_client: LrcApiClient,
}

impl AppState {
    pub fn test_fixture() -> Self {
        Self {
            playback: PlaybackState::test_fixture(),
            airplay: AirPlayState::test_fixture(),
            separation: SeparationState::test_fixture(),
            remote: RemoteState::test_fixture(),
            shell: AppShell::test_fixture(),
            lrclib_client: LrcLibClient::new_default(),
            lrcapi_client: LrcApiClient::new_default(),
        }
    }

    pub fn library_root(&self) -> Result<LibraryRoot, CommandError> {
        self.shell.library_root()
    }

    pub fn resolve_model_path(&self) -> Result<PathBuf, CommandError> {
        self.shell.resolve_model_path()
    }

    pub fn clone_for_background(&self) -> Self {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn playback_state_fixture_shares_arcs() {
        let a = PlaybackState::test_fixture();
        let b = a.clone();
        a.playback_request_id.store(99, Ordering::SeqCst);
        assert_eq!(b.playback_request_id.load(Ordering::SeqCst), 99);
    }

    #[test]
    fn app_state_fixture_constructs_all_domains() {
        let state = AppState::test_fixture();
        assert!(state.shell.library.lock().unwrap().is_none());
    }

    #[test]
    fn delegation_methods_work() {
        let state = AppState::test_fixture();
        assert!(state.library_root().is_err());
        let bg = state.clone_for_background();
        state
            .playback
            .playback_request_id
            .store(42, Ordering::SeqCst);
        assert_eq!(bg.playback.playback_request_id.load(Ordering::SeqCst), 42);
    }
}
