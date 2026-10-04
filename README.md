# 슈퍼 뿌요뿌요 2 리믹스 (SNES) 한글 패처

SFC용 《슈퍼 뿌요뿌요 2 리믹스》(す〜ぱ〜ぷよぷよ通リミックス) 일본판에 한글 패치를 적용하는 Rust 코드입니다. 원본 식별, SNES LZ 압축·해제, 스토리·엔딩·옵션 도움말·메뉴 문구 재배치, 한글 폰트와 메뉴·엔딩 그래픽 생성, 65816 코드 패치, 변경 범위 감사와 BPS 생성·검증을 제공합니다.

배포용 BPS와 적용 방법은 [뿌요뿌요 시리즈 한글화 패치](https://github.com/mcpads/puyo-puyo-kr-patch/tree/main/sfc-puyopuyo2-remix)에서 제공합니다.

## 제공하지 않는 것

이 저장소에는 원본 ROM, 패치를 적용한 ROM, 스토리 번역 JSON과 검토 기록, 폰트 파일, 한글 메뉴·랭킹·엔딩 그래픽이 없습니다. 따라서 이 저장소만으로는 배포 패치를 다시 만들 수 없습니다. 아래 입력을 직접 갖춘 경우에만 `remix-production-build`가 ROM과 BPS를 생성합니다.

## 빌드와 테스트

```bash
cargo build --release
cargo test
```

기본 테스트는 합성 입력과 코드 안의 상수만 사용합니다. 원본 ROM, 폰트, 그래픽 입력이 필요한 테스트는 `#[ignore = "requires ..."]`로 필요한 입력을 밝혀 두었습니다. 입력을 갖춘 뒤 `cargo test -- --ignored`로 실행하며, 입력이 없으면 성공으로 넘어가지 않고 실패합니다. `erasing_keeps_every_fang_and_leaves_no_lettering`은 에뮬레이터에서 캡처한 원본 메뉴 VRAM 덤프(`out/evidence/menu/`)가 추가로 필요하며 이 덤프는 배포하지 않습니다.

## 지원 원본

지원 일본판 ROM은 헤더 없는 2,097,152바이트이며 SHA-256은 다음과 같습니다. 빌드는 이 해시가 다르면 진행하지 않습니다.

```text
19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa
```

기본 경로는 `roms/Super Puyo Puyo Tsuu Remix (Japan).sfc`입니다. 일반판 《슈퍼 뿌요뿌요 2》와는 다른 ROM입니다.

## 빌드 입력

모든 입력은 저장소 루트를 기준으로 한 고정 경로에서 읽습니다.

| 입력 | 경로 | 비고 |
| --- | --- | --- |
| 스토리 번역과 검토 기록 | `assets/translations/` | `story_ko.json`, `story_review.json`, `review_state.json`, `story_port_map.json`, `story_terms.tsv`, `story_style.md` |
| 공개 미리보기 결정 기록 | `assets/release/preview.json` | `preview` 정책에서만 사용 |
| 메뉴 그래픽 | `assets/menu_graphics/` | `mode_labels/`, `remix_labels/`, `tokoton_play/`, `easy_courses/`, `game_over/`, `prompts/` |
| 랭킹 제목 그래픽 | `assets/ranking_graphics/ranking_lettering_sheet.png` | |
| 진엔딩 그래픽 | `assets/ending_graphics/true_ending_shock.png` | |
| 공용 16×16 글꼴 | `assets/fonts/galmuri14.ttf` | [Galmuri](https://github.com/quiple/galmuri) v2.40.4 |
| 쉬운 코스 캡션 글꼴 | `assets/fonts/galmuri11.ttf` | [Galmuri](https://github.com/quiple/galmuri) |
| 진엔딩 `카~ 군~!`·플레이 효과 글꼴 | `assets/fonts/galmuri11_bold.ttf` | [Galmuri](https://github.com/quiple/galmuri) Bold |
| 프롬프트·랭킹 글꼴 | `assets/fonts/bmjua.ttf` | 배달의민족 주아체 |
| 진엔딩 `앗!` 글꼴 | `assets/fonts/maplestory_bold.ttf` | [메이플스토리 서체](https://maplestory.nexon.com/Media/Font) |

폰트는 재배포 조건을 이 저장소에서 보장할 수 없어 포함하지 않습니다. 각 폰트의 라이선스는 배포처에서 확인하세요. 배포 패치 v1.0.0은 다음 폰트 파일로 만들었습니다. Galmuri 세 파일은 SHA-256이 이 값과 다르면 빌드가 진행하지 않습니다.

```text
6fe6c3fe4369e3837ac348431e8670733d67aa4bd550982baa72cc93c81a1c68  galmuri14.ttf
2c709890595668f7bdb6df408420fda957dde0288e95b31a1cc17a2ab98b4b4f  galmuri11.ttf
5265b2f437fe81f0c8095b44c0173dd9a276b58a42552bf983f21c0e69e6e8af  galmuri11_bold.ttf
e8e6aa8b1b662c7bf0d7f136f29e822e0985176458a6e5d0ba08afc4a5c901a9  bmjua.ttf
d57eaff48a793ff872a0f33bba2943d058d07c81ed64c68054858a287b85811a  maplestory_bold.ttf
```

## ROM 생성

```bash
cargo run --release -- remix-production-build \
  --rom "roms/Super Puyo Puyo Tsuu Remix (Japan).sfc" \
  --policy preview
```

`--policy`는 입력 검토 상태에 따라 빌드 허용 여부와 ROM 안의 표식을 정합니다.

| 정책 | 조건 | ROM 안 표식 |
| --- | --- | --- |
| `poc` | 번역이 검토 대기 이상 | 비배포 표식 |
| `pre-release` | 번역이 검토 대기 이상 | 비배포 표식 |
| `preview` | 위 조건과 `assets/release/preview.json` | 없음 |
| `release` | `review_state.json`의 모든 단위가 승인과 런타임 확인을 마침 | 없음 |

`preview.json`에는 `version`(`MAJOR.MINOR.PATCH`), `approved_by`, `approved_on`, `decision`과 비어 있지 않은 `known_limitations` 배열이 있어야 합니다. 출력은 `out/release/v<version>/`에 `puyopuyo2_remix_kr_v<version>.sfc`, `.bps`와 보고서 JSON으로 생깁니다. 빌드는 최종 ROM의 변경이 등록된 쓰기 범위 안에 있는지 확인하고, BPS를 원본에 다시 적용해 출력 ROM과 같을 때만 파일을 씁니다.

## 그 밖의 명령

`info`, `story-scan`, `story-kr-poc`, `remix-integrated-kr-poc`, `resource-inventory` 등 조사·부분 빌드 명령의 사용법은 `cargo run -- help <명령>`으로 확인할 수 있습니다. 일부 명령은 에뮬레이터에서 캡처한 VRAM 덤프나 일반판 《슈퍼 뿌요뿌요 2》 ROM을 입력으로 받습니다.

## 라이선스

이 저장소의 소스 코드는 [MIT License](LICENSE)로 제공합니다.
