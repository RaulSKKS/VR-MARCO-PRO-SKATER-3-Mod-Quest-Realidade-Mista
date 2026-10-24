# Salas online (código de 6 letras) — Worker da Cloudflare

Um Worker pequeno que deixa os jogadores se acharem por um código curto (ex.: `K7M2QX`), sem digitar IP nem abrir
porta no roteador. Ele só apresenta os jogos uns aos outros: os dados do jogo vão direto entre os jogadores, ou pelo
relay TURN da Cloudflare quando a conexão direta não passa. Adaptado do Worker do Tenkaichi3Decomp, para salas de
2 a 10 skatistas (o host repassa os pacotes de todos, como no multiplayer que o port já tinha).

Arquivos: `worker.js` (o Worker), `schema.sql` (as tabelas), `test.mjs` (testes, `node matchmaking/test.mjs`),
`dev-server.mjs` (o Worker rodando neste PC, para testar sem publicar).

Licença: `worker.js` é **AGPL-3.0** (adaptado do Worker do Tenkaichi3Decomp, que é AGPL-3.0). Quem rodar uma
versão alterada para outras pessoas tem que oferecer o código dela.

## Publicar (uma vez, no painel da Cloudflare, conta grátis, sem domínio)

1. **Banco**: menu *Storage & Databases → D1 SQL Database → Create* → nome `skate-rooms` → *Create*.
   Abra o banco → aba *Console* → cole e rode **uma linha de cada vez** as três linhas de `schema.sql`.
2. **Worker**: menu *Compute (Workers) → Workers & Pages → Create → Start with Hello World* → nome `skate-rooms` →
   *Deploy*. Depois *Edit code*, apague tudo, cole o conteúdo de `worker.js` e clique em *Deploy*.
3. **Ligar o banco ao Worker**: no Worker → *Bindings → Add binding → D1 database* → nome da variável `DB`, banco
   `skate-rooms` → *Add binding*.
4. Abra o endereço do Worker (`https://skate-rooms.<sua-conta>.workers.dev`): deve aparecer
   "Skate 3 port matchmaking: room codes...". Esse endereço é o que vai dentro do jogo.

### Relay TURN (opcional, para quem não conecta direto: 4G, roteador travado)

5. Menu *Realtime → TURN Server → Create* → nome `skate` → copie o **Token ID** e o **API Token**.
6. No Worker → *Settings → Variables and Secrets → Add* → tipo **Secret**: `TURN_KEY_ID` = Token ID, e
   `TURN_KEY_TOKEN` = API Token. *Deploy*.

Grátis até 1.000 GB/mês. Para o Worker medir o uso e desligar o relay antes de passar do limite (500 GB por padrão,
variável `TURN_MONTHLY_GB`):

7. Copie o **Account ID** (página inicial da conta, coluna da direita) → variável comum `CF_ACCOUNT_ID`.
8. *My Profile → API Tokens → Create Token → Custom* com a permissão *Account → Account Analytics → Read* →
   secret `CF_ANALYTICS_TOKEN`.
9. Secret `ADMIN_KEY`: um texto aleatório de 16+ letras (para consultar `/v1/status`).
10. No Worker → *Settings → Trigger Events → Cron Triggers → Add* → `*/5 * * * *`.

Sem os passos 7–10 o relay funciona, mas só com o limite de logins por dia (500, variável `TURN_DAILY_MAX`).
Com eles configurados, nenhum login é dado enquanto a medição não tiver funcionado pelo menos uma vez.

Ver o uso: `curl -X POST https://skate-rooms.<sua-conta>.workers.dev/v1/status -d '{"admin":"<ADMIN_KEY>"}'`

## Colocar o endereço no jogo

- PC: `--net-match-url https://skate-rooms.<sua-conta>.workers.dev` ou a variável de ambiente `SK8_MATCH_URL`.
- Quest (e PC sem opção): defina `SK8_MATCH_URL` **ao compilar** (o endereço fica dentro do APK), ou coloque-o em
  `DEFAULT_URL` em `crates/skate-net/src/rooms.rs`.

## Jogar

- Criar sala: `--net-room-host [2-10]` (4 se não disser), ou no Quest: menu → *Online* → *Criar sala*. O código
  aparece no HUD/painel.
- Entrar: `--net-room-join K7M2QX`, ou no Quest: *Online* → *Entrar* → escolher as 6 letras com o analógico.
- `--net-name Marco` muda o nome mostrado aos outros.

## Testar sem publicar

```
node matchmaking/dev-server.mjs
```
e abra o jogo com `--net-match-url http://<IP deste PC>:8787` (o Quest na mesma rede Wi-Fi também alcança).
Teste de ponta a ponta (três jogos se acham pelo servidor local):
`SK8_MATCH_URL=http://127.0.0.1:8787 cargo test -p skate-game --lib three_skaters -- --ignored`

## API (JSON por POST)

| Caminho | Pedido | Resposta |
|---|---|---|
| `/v1/host` | `v, rel, name, addrs, max` | `code, key, you` |
| `/v1/join` | `v, rel, code, name, addrs` | `key, slot, you, host{name, addrs}` |
| `/v1/poll` | `code, key` (host; mantém a sala viva) | `members[{slot, name, addrs}]` |
| `/v1/poll` | `code, key` (convidado) | `host{name, addrs}` (o `relay=` aparece aqui) |
| `/v1/addrs` | `code, key, addrs` (host) | `{}` |
| `/v1/relay` | `code, key` (host, com convidado) | `turn{server, username, credential}` ou `noRelay` |
| `/v1/leave` | `code, key` | `{}` (host fecha a sala; convidado libera a vaga) |
| `/v1/hello` | `rel` | `min, latest, now` |

A sala some 30 s depois do último `poll` do host (e no máximo em 6 h). Limites por IP: 4 salas abertas, 20 salas
por hora, 30 códigos errados por hora, 10 logins de relay por dia. `MIN_RELEASE` recusa versões antigas do jogo.
