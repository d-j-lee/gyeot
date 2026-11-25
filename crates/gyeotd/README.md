# gyeotd

곁 데몬 실행 파일. 콘솔 프로그램이다(셸이나 Windows 예약 작업이 창 없이 띄운다).

```
gyeotd                 # Ctrl+C까지 서버를 돌린다(시작할 때 "얼굴 열기: gyeotd --open"을 보인다)
gyeotd --open          # 실행 중인 데몬을 확인하고 일회용 열기 코드를 받아 기본 브라우저로 얼굴을 연다
gyeotd --open --print  # 열지 않고 주소만 표준 출력에 보인다(120초 안에 한 번 쓸 수 있다)
gyeotd --init          # 데이터 폴더와 토큰을 만들고 경로를 보인다
gyeotd --print-config  # 실제로 쓰는 설정을 TOML로 보인다
gyeotd --version
```

`--open`(계약 §12.1·§13.1):

1. 토큰을 보내기 전에 `GET /v1/hello?n=<난수>`로 그 포트의 서버가 내 토큰을 아는지 확인한다. 증명이 맞지 않으면 "이 포트의 서버는 내 곁 데몬이 아니에요(다른 사용자나 프로그램일 수 있어요)"를 보이고 exit 1(토큰을 보내지 않는다).
2. `POST /v1/boot`(토큰)로 일회용 주소를 받는다.
3. 주소를 프로그램 인자에 싣지 않는다(다른 사용자가 `ps`·`/proc`로 읽을 수 있다). `<데이터>/open/open-<난수>.html`(폴더 0700, 파일 0600)에 그 주소로 넘어가는 쪽을 쓰고, 그 파일 경로로 브라우저를 연다(Windows `rundll32 url.dll,FileProtocolHandler`, Linux `xdg-open`, macOS `open`). 5분 넘은 `open-*.html`은 실행마다 지운다.

로그는 표준 오류와 `<데이터>/gyeotd.log`(Unix 0600)에 남는다(5 MiB를 넘으면 시작할 때 비운다). 자세히 보려면 `RUST_LOG=debug`.
