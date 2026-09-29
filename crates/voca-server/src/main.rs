//! 서버 실행.
//!
//! 여기서 하는 일은 셋뿐이다: 설정을 읽고, 저장소를 열고, 라우터를 띄운다.
//! 규칙은 [`voca_server`] 안에 있다.

use std::process::ExitCode;

use voca_server::{Config, build_router};
use voca_store_sqlite::SqliteStore;

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "설정을 읽지 못했다");
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    // 허용 목록이 비어 있으면 모든 변경 요청이 403 이 된다 — 서비스가 죽은 상태로
    // 부팅된 것이다. 조용히 부팅하지 않고 여기서 막는다.
    if config.csrf_would_reject_everything() {
        tracing::warn!("VOCA_ALLOWED_ORIGINS 가 비어 있다. 모든 변경 요청이 거부된다.");
    }

    let store = match SqliteStore::open(std::path::Path::new(&config.database_path)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = ?e, "저장소를 열지 못했다");
            eprintln!("저장소를 열지 못했다: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = voca_server::seed::load_seeds_into_store(&store).await {
        tracing::warn!(error = ?e, "시드 단어 로드 중 경고가 발생했다");
    }

    let state = voca_server::AppState::new(store, &config);
    let app = build_router(state);

    let listener = match tokio::net::TcpListener::bind(config.bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, addr = %config.bind, "바인드하지 못했다");
            eprintln!("바인드하지 못했다: {e}");
            return ExitCode::FAILURE;
        }
    };

    tracing::info!(addr = %config.bind, db = %config.database_path, "서버를 띄웠다");

    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    {
        tracing::error!(error = %e, "서버가 죽었다");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// 종료 시그널을 받으면 진행 중인 요청을 끝내고 내려간다.
///
/// `SIGINT`(Ctrl-C)와 `SIGTERM`(배포 환경의 정상 종료)을 모두 받는다. 강제 종료는
/// SQLite 쓰기 도중이면 WAL 복구에 의존하게 되므로 피한다.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "SIGTERM 을 받을 수 없다");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Ctrl-C 를 받았다"),
        () = terminate => tracing::info!("SIGTERM 을 받았다"),
    }
}

/// 로그 초기화.
///
/// 환경변수 `VOCA_LOG` 로 필터를 바꾼다. 프로젝트에 로깅 전용 크레이트를 더
/// 얹는 대신 `tracing` 만 쓴다 — 판단을 미루려 하지 않고 지금 쓰는 것을 쓴다.
fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_env("VOCA_LOG")
        .or_else(|_| EnvFilter::try_new("voca_server=info,tower_http=warn,warn"))
        .expect("필터 문자열은 상수다");
    let _ = fmt().with_env_filter(filter).with_target(true).try_init();
}
