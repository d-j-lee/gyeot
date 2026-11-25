# 실측 기록

`docs/roadmap.md`의 F 목록을 실제로 확인한 기록을 여기에 먼저 적는다. 문서와 README의 상태 표는 이 기록을 근거로 고친다. 날짜·환경·무엇을 봤는지·결과를 적는다.

## 2026-11-26 · 개발 컨테이너(Linux x86_64, Claude Code 2.1.332)

- **끝-끝(가짜 없음)**: `tests/e2e/e2e.mjs`로 실제 `gyeotd`·`gyeot-hook`·`gyeot-channel.mjs`(MCP SDK 클라이언트가 Claude Code 역할)·얼굴(Chrome 헤드리스)을 이었다. 39/39 통과.
  - 공식 문서의 페이로드 꼴로 만든 Claude Code 훅 입력으로 승인 대기 → `approval`(95) → 같은 세션에 방금 입력이 있으면 `ambient`, 다른 세션이면 `nudge`. 같은 도구의 `PostToolUse`로 닫힘.
  - 질문으로 끝난 `Stop` → `decision`. `/v1/say` → `notifications/claude/channel` 도착(meta 키 식별자, `from=dj`·`via=gyeot`). `reply` → 얼굴 SSE `reply`. `request_decision` → 선택지 카드 → 선택이 `[선택]` 메시지로 같은 세션에 돌아옴.
  - 하네스 `gate` `block` → `failed`, 존재감 `alert`. 데몬 재시작 뒤 항목이 재생으로 같음. 프롬프트 본문은 로그에 없음. 데몬이 꺼져 있을 때 훅은 5ms 안에 exit 0, stdout 비어 있음, 스풀에 남고 데몬이 다시 뜨면 비워짐.
  - pid 연결: 이 시험에서는 훅과 channel의 부모가 같은 프로세스라 `pid_hint == ppid`로 이어졌다. **실제 Claude Code에서 이렇게 되는지는 F1로 남는다.**
- **실제 Claude Code 훅 실행 시도(실패, 환경 탓)**: 이 컨테이너 안에서 `claude -p --settings …`로 곁 훅을 건 중첩 세션을 띄웠다. 인증 오류("Authentication error")로 턴이 끝났고, 셸 형식 기록 훅을 같이 걸어도 아무 훅도 실행되지 않았다. 이 컨테이너는 관리되는 하위 세션 환경(`CLAUDE_CODE_CHILD_SESSION`, policy-limits)이라 중첩 세션의 훅·인증이 막힌 것으로 본다[추론]. **실제 훅 페이로드 대조는 사용자 Windows에서 F1과 함께 한다.** 정규화는 공식 hooks 레퍼런스(2026-11-26 열람)의 필드 이름을 따랐다.
