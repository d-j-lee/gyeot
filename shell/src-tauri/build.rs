fn main() {
    // 앱 명령마다 `allow-<명령>`·`deny-<명령>` 권한을 만든다. 앱 매니페스트가 있으면
    // 어떤 출처(로컬·원격)든 capability로 허락한 명령만 부를 수 있다.
    // capabilities/overlay-remote.json 은 데몬 페이지(127.0.0.1:47321)에 앞의 두 명령만 준다.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "set_hit_rect",
            "set_expanded",
            "retry_connect",
        ]),
    ))
    .expect("tauri-build 실패");
}
