# Skate 3 AR — Marco VR

Código completo do Skate 3 AR na versão do canal **VR Tutoriais Marco Pereira**, para Meta Quest 3 (realidade
mista) e PC. A versão atual está em `crates/skate-game/src/version.rs` e aparece no menu do jogo.

> Os arquivos do Skate 3 não estão neste repositório. Cada jogador converte o próprio disco (ISO de Xbox 360);
> o kit de instalação faz isso. Os assets do Marco (texturas, vídeos, pistas, menus) estão aqui e vão embutidos
> no APK na compilação.

## O que esta versão acrescenta

- **Marca Marco VR**: roupa do Marco no skatista (galáxia e preta), Quest 3 na cabeça, estátua, logo, cartões
  de ajuda e painel de menu (`crates/skate-game/src/xr/marcovr.rs`, `tools/make_marcovr_assets.py`).
- **Pista do Marco** em realidade mista, com mapas, barreira, telão e painéis de LED (`xr/build/park*.rs`,
  `xr/tv.rs`). O telão toca os vídeos do canal e alterna com o placar; uma tela vertical toca o reels
  (`tools/make_tv_video.py`).
- **Online por código de sala** para até 10 jogadores, no Quest e no PC:
  - Worker da Cloudflare com banco D1 que entrega os códigos (`matchmaking/`);
  - conexão direta por STUN e hole punching, com relay TURN opcional (`crates/skate-net/src/{rooms,stun,turn}.rs`,
    `crates/skate-game/src/multiplayer/rooms.rs`);
  - página Online no menu do Quest com nome, lista da sala e nomes sobre os skatistas (`xr/online.rs`);
  - o host manda na pista e os convidados espelham; as posições viajam relativas à pista (`xr/build/sync.rs`,
    `SharedFrame` em `multiplayer.rs`);
  - roupa do Marco e pontuação de cada um sincronizadas, com ranking no telão.
- Limite de tamanho da pista (o jogo travava perto do tamanho real) e versão visível nos menus.

## Compilar

- **PC**: `BUILD.bat` e depois `PLAY.bat` (Rust com MSVC e LLVM; ver `docs/README-original.md`).
- **Quest**: `scripts/quest/build-apk.sh` (Android SDK e NDK, `cargo-apk`; ver `scripts/quest/README.md`).
  O endereço do Worker fica em `crates/skate-net/src/rooms.rs` (`DEFAULT_URL`), ou na variável
  `SK8_MATCH_URL` na hora de compilar.
- **Worker**: passo a passo em `matchmaking/README.md`; testes com `node matchmaking/test.mjs`.
- Teste de ponta a ponta do online: `SK8_MATCH_URL=<url> cargo test -p skate-game --lib three_skaters -- --ignored`.

## De onde veio (referências)

- **Skate 3 AR for Meta Quest 3**, de Andrew Nakas: <https://github.com/andrewnakas/skate3-quest-ar>. É a base
  deste código (o port para Quest, a construção de peças, o som limpo). O README original está em
  `docs/README-original.md`.
- **Skate 3 Rust Engine**: <https://github.com/SK8-ENGINE/skate-3-rust-engine>. É o motor do jogo (física, manobras,
  animação, mapas `.skate`, o multiplayer original de 10 jogadores e o conversor de assets).
- **skate3-cleanroom**: <https://github.com/andrewnakas/skate3-cleanroom>, com o modelo, as texturas e os sons
  regenerados.
- **Tenkaichi3Decomp** (port de PC do Dragon Ball Z Budokai Tenkaichi 3): o Worker de salas (`matchmaking/worker.js`)
  foi adaptado do Worker de matchmaking dele, e o cliente TURN segue o comportamento do relay dele.
- **XboxDev/extract-xiso**: <https://github.com/XboxDev/extract-xiso>, o extrator de ISO usado pelo conversor.
- Bibliotecas Rust e as cópias corrigidas da Bevy em `vendor/`, cada uma com a sua licença.

## Licença

O projeto segue a **GPL-3.0** do código de origem (`LICENSE`). O arquivo `matchmaking/worker.js`, por ter sido
adaptado do Tenkaichi3Decomp, fica sob a **AGPL-3.0**: quem rodar uma versão alterada dele para outras pessoas tem
que oferecer o código. Os vídeos e as imagens do canal VR Tutoriais Marco Pereira pertencem ao canal. Projeto não
oficial, sem ligação com a EA. Os arquivos do Skate 3 nunca são distribuídos.
