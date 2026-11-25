# gyeotd

곁 데몬 실행 파일. 콘솔 프로그램이다(셸이나 Windows 예약 작업이 창 없이 띄운다).

```
gyeotd                 # Ctrl+C까지 서버를 돌린다
gyeotd --init          # 데이터 폴더와 토큰을 만들고 경로를 보인다
gyeotd --print-config  # 실제로 쓰는 설정을 TOML로 보인다
gyeotd --version
```

로그는 표준 오류와 `<데이터>/gyeotd.log`에 남는다(5 MiB를 넘으면 시작할 때 비운다). 자세히 보려면 `RUST_LOG=debug`.
