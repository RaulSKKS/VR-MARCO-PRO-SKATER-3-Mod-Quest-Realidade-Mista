# Skate 3 AR — Marco VR (Meta Quest 3)

Skate 3 em realidade mista no Quest 3: o skatista anda na sua sala, nos móveis e na pista do Marco, com
rampas, corrimãos e a estátua que você coloca com os controles. Dá para jogar online com amigos por um
código de sala.

**Versão:** veja `VERSAO.txt`. A mesma versão aparece no canto de cima do menu dentro do jogo.

> **Os arquivos do Skate 3 não estão aqui.** Você precisa ter o seu próprio disco do Skate 3 de
> Xbox 360, em ISO. A conversão roda no seu PC e o resultado fica só com você.

## O que precisa

- Windows 10 ou 11 com internet na primeira vez.
- A ISO do **seu** Skate 3 de Xbox 360.
- Um Meta Quest 3 em modo desenvolvedor (app Meta Horizon no celular > Dispositivos > Modo de
  desenvolvedor) e um cabo USB.
- Uns 10 GB livres no disco durante a conversão.

## Instalar

**Jeito mais fácil:** coloque a ISO em `COLOQUE-SUA-ISO-AQUI`, conecte o Quest e dê dois cliques em
**`INSTALAR-TUDO.bat`**. Ele faz os dois passos abaixo em sequência.

Tudo do Marco (roupa, estátua, pistas, telões, vídeos, menu e online) já vem dentro do APK. Da sua ISO
saem só os arquivos do Skate 3 (mapas, skatista, animações), que não podemos distribuir.

Passo a passo:

1. Baixe este repositório (botão **Code > Download ZIP**) e extraia numa pasta.
2. Coloque a ISO do Skate 3 na pasta **`COLOQUE-SUA-ISO-AQUI`**.
3. Dê dois cliques em **`1-PREPARAR-JOGO.bat`**. Ele baixa o conversor e o extrator, extrai a ISO e
   converte mapas e skatista. A primeira vez demora; deixe a janela aberta até aparecer **Pronto!**
4. Ligue o Quest, conecte no cabo e dê dois cliques em **`2-INSTALAR-NO-QUEST.bat`**. Se aparecer
   "Permitir depuração USB?" no óculos, marque **Sempre permitir** e confirme.
5. No Quest: **Biblioteca > Fontes desconhecidas > Skate 3 AR**.

Para atualizar para uma versão nova, baixe o kit de novo e rode só o passo 2 (os dados do passo 1
continuam em `_trabalho`, se você copiar essa pasta junto).

## Jogar

- **Menu** (toque no botão de menu do controle esquerdo): pista do Marco na sua frente, mapa, tamanho,
  barreira, roupa do Marco, modo construção, ajuda e **Online**.
- **Online: jogar com amigos**: escolha o seu nome, crie uma sala (2 a 10 jogadores) e passe o código de
  6 letras, ou entre com o código de um amigo. Quem cria a sala manda na pista (mapa, barreira e peças);
  os skatistas ficam juntos na pista mesmo que cada um a coloque num canto diferente da sala.
- **Telão**: vídeos do canal VR Tutoriais Marco Pereira, alternando com o placar (no online, quem está
  na frente).
- Analógico esquerdo: remar e virar · A: ollie · analógico direito: manobras · gatilhos no ar: grab ·
  Y: descer/subir no skate. Vire o controle esquerdo para o rosto para ver a ajuda dos botões.

## Problemas comuns

- **"Nao achei a ISO"**: o arquivo precisa terminar em `.iso` e estar dentro de `COLOQUE-SUA-ISO-AQUI`.
- **"Nenhum Quest encontrado"**: confira o modo desenvolvedor e o cabo. Se o Windows não reconhecer o
  óculos, instale o driver "Oculus ADB Driver" (developers.meta.com) ou o SideQuest.
- **Conexão online não fecha**: em 4G/5G ou roteador muito fechado a conexão direta pode não passar.

## Código-fonte

O código completo desta versão, que gera o APK da pasta `apk`, está em [`codigo-fonte/`](codigo-fonte/), com as
instruções para compilar e as referências de onde cada parte veio. Quem só quer jogar não precisa abrir essa
pasta.

## De onde vem

Feito sobre o [Skate 3 AR](https://github.com/andrewnakas/skate3-quest-ar) e o Skate 3 Rust Engine
(GPL-3.0), com a marca e as mudanças do canal VR Tutoriais Marco Pereira (online por código, pista do
Marco, telões). O conversor e o extrator de ISO (XboxDev/extract-xiso) são baixados dos lançamentos
oficiais deles e conferidos pelo hash. Projeto não oficial, sem ligação com a EA.
