use crate::errors::{stream_error, Result};
use crate::models::dto::ManagerState;
use crate::models::Manager;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

#[derive(Debug, Default)]
struct State {
    peers: Vec<Option<UnixStream>>,
    last_state: String, //last_state: String
}

#[derive(Debug, Default)]
pub struct StateSocket {
    state: Arc<Mutex<State>>,
    listener: Option<tokio::task::JoinHandle<()>>,
    socket_file: PathBuf,
}

impl Drop for StateSocket {
    fn drop(&mut self) {
        if !std::thread::panicking() && self.listener.is_some() {
            panic!("StateSocket has to be shutdown explicitly before drop");
        }
    }
}

impl StateSocket {
    /// Bind to Unix socket and listen.
    /// # Errors
    ///
    /// Will error if `build_listener()` cannot be unwrapped or awaited.
    /// As in `build_listener()`, this is likely a filesystem issue,
    /// such as incorrect permissions or a non-existant file.
    pub async fn listen(&mut self, socket_file: PathBuf) -> Result<()> {
        self.socket_file = socket_file;
        let listener = self.build_listener().await?;
        self.listener = Some(listener);
        Ok(())
    }

    /// Explicitly shutdown `StateSocket` to perform cleanup.
    pub async fn shutdown(&mut self) {
        if let Some(listener) = self.listener.take() {
            listener.abort();
            listener.await.ok();
            fs::remove_file(self.socket_file.as_path()).await.ok();
        }
    }

    /// # Errors
    /// Will return Err if a mut ref to the peer is unavailable.
    /// Will return error if state cannot be serialized
    pub async fn write_manager_state(&mut self, manager: &Manager) -> Result<()> {
        if self.listener.is_some() {
            let state: ManagerState = manager.into();
            let mut json = serde_json::to_string(&state)?;
            json.push('\n');
            let mut state = self.state.lock().await;
            if json != state.last_state {
                state.peers.retain(std::option::Option::is_some);
                for peer in &mut state.peers {
                    if peer
                        .as_mut()
                        .ok_or_else(stream_error)?
                        .write_all(json.as_bytes())
                        .await
                        .is_err()
                    {
                        peer.take();
                    }
                }
                state.last_state = json;
            }
        }
        Ok(())
    }

    async fn build_listener(&self) -> Result<tokio::task::JoinHandle<()>> {
        let state = self.state.clone();
        let listener = if let Ok(m) = UnixListener::bind(&self.socket_file) {
            m
        } else {
            fs::remove_file(&self.socket_file).await?;
            UnixListener::bind(&self.socket_file)?
        };
        Ok(tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut peer, _)) => {
                        let mut state = state.lock().await;
                        if peer.write_all(state.last_state.as_bytes()).await.is_ok() {
                            state.peers.push(Some(peer));
                        }
                    }
                    Err(e) => log::error!("accept failed = {:?}", e),
                }
            }
        }))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::utils::helpers::test::temp_path;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[test]
    fn multiple_peers() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(multiple_peers_async());
    }
    async fn multiple_peers_async() {
        let manager = Manager::new_test();

        let socket_file = temp_path().await.unwrap();
        let mut state_socket = StateSocket::default();
        state_socket.listen(socket_file.clone()).await.unwrap();
        state_socket.write_manager_state(&manager).await.unwrap();

        assert_eq!(
            serde_json::to_string(&Into::<ManagerState>::into(&manager)).unwrap(),
            BufReader::new(UnixStream::connect(socket_file.clone()).await.unwrap())
                .lines()
                .next_line()
                .await
                .expect("Read next line")
                .unwrap()
        );

        assert_eq!(
            serde_json::to_string(&Into::<ManagerState>::into(&manager)).unwrap(),
            BufReader::new(UnixStream::connect(socket_file.clone()).await.unwrap())
                .lines()
                .next_line()
                .await
                .expect("Read next line")
                .unwrap()
        );

        assert_eq!(
            serde_json::to_string(&Into::<ManagerState>::into(&manager)).unwrap(),
            BufReader::new(UnixStream::connect(socket_file).await.unwrap())
                .lines()
                .next_line()
                .await
                .expect("Read next line")
                .unwrap()
        );

        state_socket.shutdown().await;
    }

    #[test]
    fn get_update() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(get_update_async());
    }
    async fn get_update_async() {
        let manager = Manager::new_test();

        let socket_file = temp_path().await.unwrap();
        let mut state_socket = StateSocket::default();
        state_socket.listen(socket_file.clone()).await.unwrap();
        state_socket.write_manager_state(&manager).await.unwrap();

        let mut lines = BufReader::new(UnixStream::connect(socket_file).await.unwrap()).lines();

        assert_eq!(
            serde_json::to_string(&Into::<ManagerState>::into(&manager)).unwrap(),
            lines.next_line().await.expect("Read next line").unwrap()
        );

        // Fake state update.
        state_socket.state.lock().await.last_state = String::default();
        state_socket.write_manager_state(&manager).await.unwrap();

        assert_eq!(
            serde_json::to_string(&Into::<ManagerState>::into(&manager)).unwrap(),
            lines.next_line().await.expect("Read next line").unwrap()
        );

        state_socket.shutdown().await;
    }

    #[test]
    fn socket_cleanup() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(socket_cleanup_async());
    }
    async fn socket_cleanup_async() {
        let socket_file = temp_path().await.unwrap();
        let mut state_socket = StateSocket::default();
        state_socket.listen(socket_file.clone()).await.unwrap();
        state_socket.shutdown().await;
        assert!(!socket_file.exists());
    }

    #[test]
    fn socket_already_bound() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(socket_already_bound_async());
    }
    async fn socket_already_bound_async() {
        let socket_file = temp_path().await.unwrap();
        let mut old_socket = StateSocket::default();
        old_socket.listen(socket_file.clone()).await.unwrap();
        assert!(socket_file.exists());
        let mut state_socket = StateSocket::default();
        state_socket.listen(socket_file.clone()).await.unwrap();
        state_socket.shutdown().await;
        assert!(!socket_file.exists());
        old_socket.shutdown().await;
    }
}
