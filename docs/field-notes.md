# 실측 기록

`docs/roadmap.md`의 F 목록을 실제로 확인한 기록을 여기에 먼저 적는다. 문서와 README의 상태 표는 이 기록을 근거로 고친다. 날짜·환경·무엇을 봤는지·결과를 적는다.

## 2026-11-26 · 개발 컨테이너(Linux x86_64, Claude Code 2.1.332)

- **끝-끝(가짜 없음)**: `tests/e2e/e2e.mjs`로 실제 `gyeotd`·`gyeot-hook`·`gyeot-channel.mjs`(MCP SDK 클라이언트가 Claude Code 역할)·얼굴(Chrome 헤드리스)을 이었다. 39/39 통과.
  - 공식 문서의 페이로드 꼴로 만든 Claude Code 훅 입력으로 승인 대기 → `approval`(95) → 같은 세션에 방금 입력이 있으면 `ambient`, 다른 세션이면 `nudge`. 같은 도구의 `PostToolUse`로 닫힘.
  - 질문으로 끝난 `Stop` → `decision`. `/v1/say` → `notifications/claude/channel` 도착(meta 키 식별자, `from=dj`·`via=gyeot`). `reply` → 얼굴 SSE `reply`. `request_decision` → 선택지 카드 → 선택이 `[선택]` 메시지로 같은 세션에 돌아옴.
  - 하네스 `gate` `block` → `failed`, 존재감 `alert`. 데몬 재시작 뒤 항목이 재생으로 같음. 프롬프트 본문은 로그에 없음. 데몬이 꺼져 있을 때 훅은 5ms 안에 exit 0, stdout 비어 있음, 스풀에 남고 데몬이 다시 뜨면 비워짐.
  - pid 연결: 이 시험에서는 훅과 channel의 부모가 같은 프로세스라 `pid_hint == ppid`로 이어졌다. **실제 Claude Code에서 이렇게 되는지는 F1로 남는다.**
- **실제 Claude Code 훅 실행 시도(실패, 환경 탓)**: 이 컨테이너 안에서 `claude -p --settings …`로 곁 훅을 건 중첩 세션을 띄웠다. 인증 오류("Authentication error")로 턴이 끝났고, 셸 형식 기록 훅을 같이 걸어도 아무 훅도 실행되지 않았다. 이 컨테이너는 관리되는 하위 세션 환경(`CLAUDE_CODE_CHILD_SESSION`, policy-limits)이라 중첩 세션의 훅·인증이 막힌 것으로 본다[추론]. **실제 훅 페이로드 대조는 사용자 Windows에서 F1과 함께 한다.** 정규화는 공식 hooks 레퍼런스(2026-11-26 열람)의 필드 이름을 따랐다.

## 2026-11-26 · GitHub Actions windows-latest(실행 45686164909, 커밋 e763a11)

- 모든 작업 통과: rust·channel·e2e(ubuntu·windows), face, shell(windows), scripts(PowerShell 5.1·7), windows dist. 산출물 `gyeot-windows-x64`(zip, 6,757,409바이트), `gyeot-shell-setup-x64`(NSIS, 2,561,628바이트), `gyeot-shell-exe`.
- Windows 끝-끝 39/39. 실제 Windows에서 데몬·훅 심·channel·얼굴(Chrome)이 이어진 것은 CI 러너까지다. 사용자 PC의 WebView2·트레이·클릭 통과는 아직이다(F5~F7).
- **발견**: 데몬이 꺼져 있을 때 훅이 Windows에서 430ms 걸렸다(Linux 5ms). Windows는 닫힌 루프백 포트로의 연결이 바로 실패하지 않아 통신 한도 400ms를 다 쓴 것으로 본다[추론]. 훅은 `async`라 세션을 붙잡지 않지만, connect 단계만 따로 짧게 자르기로 했다(R1 수정에 함께 넣음).

## 2026-11-26 · 독립 적대 검증 R1(개발 컨테이너, 커밋 e763a11)

작업 대화를 물려받지 않은 검증 에이전트가 사본에서 v0.1을 공격했다. 발견 12건 가운데 높음 2·중간 4는 코드를 읽어 같은 경로를 다시 따라가 채택했다. 고친 내용은 `docs/contracts.md` §12, 결정은 G-023~G-027이다.

| 등급 | 발견 | 무엇이 깨졌나 |
|---|---|---|
| 높음 | H1 `?api=` | 얼굴이 주소의 `api` 값을 믿고 그쪽으로 토큰을 보냈다. 링크 하나로 토큰이 다른 출처에 샌다 |
| 높음 | H2 활동 훅 없음 | 기본 설치에는 승인 뒤 신호가 없어, 사람이 이미 승인한 세션에 2분 뒤 소리 알림이 났다 |
| 중간 | M1 `GET /` meta 토큰 | 인증 없이 얼굴 HTML에 토큰을 실어, 같은 PC의 다른 OS 사용자도 토큰을 얻는다 |
| 중간 | M2 Tauri 출처 허용 | `http://tauri.localhost` 등을 허용해, 루프백 80번에서 그 이름으로 뜬 페이지가 API를 쓸 수 있었다 |
| 중간 | M3 스풀 원문 | 데몬이 꺼져 있으면 프롬프트 본문·파일 내용·비밀이 스풀에 0644로 쌓였다 |
| 중간 | M4 `npm ci` | channel이 깨끗한 설치에서 의존성 충돌(이미 CI 1차에서 고침) |
| 낮음 | L1~L6 | 아무 chat_id로 답 주입·집중 모드 무시, 잘못된 Host의 기본값 처리, 클릭재킹, e2e 빌드 경로, 병합 정규식, 큰 stdin |

수정 뒤의 판정(R2)과 측정은 아래에 이어 적는다.
