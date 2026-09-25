# CODENOTCH LINUX CONTEXT

Este documento é um snapshot técnico completo e autocontido do estado atual do port Linux (Debian) do Codenotch. O objetivo é permitir que engenheiros e agentes continuem o desenvolvimento imediatamente, sem perda de contexto.

---

## 1. Projeto original

* **Nome e finalidade:** Codenotch é uma aplicação que exibe uma "notch" preta na borda da tela com indicadores visuais de consumo (quota/limite de uso) de assistentes de IA para código.
* **Upstream original:** Codenotch para macOS, construído em Swift/AppKit.
* **Implementação Windows usada como base:** O port Linux aproveita a arquitetura baseada em Rust + Tauri (v2) originalmente implementada no diretório `windows/codenotch/`, compilando-a nativamente para Linux.
* **Licença:** MIT
* **Relação deste repositório com o upstream:** Este repositório é um fork focado em adicionar, estabilizar e empacotar o suporte oficial da comunidade para distribuições Linux.

## 2. Ambiente Linux alvo

O ambiente de referência documentado e testado para este port é rigorosamente o seguinte:

* **OS:** Debian GNU/Linux 13 (Trixie)
* **Arquitetura:** amd64 / x86_64
* **Interface:** GNOME
* **Sessão:** Wayland
* **Compatibilidade X11:** Uso forçado do XWayland (via `GDK_BACKEND=x11`) quando necessário para posicionamento de janelas, devido às restrições do GNOME Mutter.
* **Runtime Web:** WebKitGTK 4.1
* **Demais dependências:** `libayatana-appindicator3-dev` (bandeja do sistema), `librsvg2-dev`, `libssl-dev`, `libxdo-dev` (para fallback de `xdotool` no X11).

## 3. Git / versão atual

* **Repositório atual:** Fork Linux/Debian.
* **Branches relevantes:** `main` (branch principal consolidada) e histórico vindo da branch `feat/linux-debian`.
* **Commit principal:** `ee41597 (HEAD -> main, origin/main) feat: merge Linux Debian support`
* **Release:** Existe um release "Linux Preview 0.1.0" no formato padrão do projeto.
* **Arquivos principais adicionados/modificados:** 
  * `linux/README.md` e seção Linux no `README.md` raiz.
  * Scripts em `linux/bin/` (`codenotch-control`, `codenotch-ide-watch`).
  * Serviço Systemd em `linux/systemd/codenotch-ide-watch.service`.
  * Código Rust em `windows/codenotch/src/` com conditionally compiled blocks `#[cfg(target_os = "linux")]`.

## 4. Arquitetura do port Linux

A estrutura lógica reaproveita o núcleo do backend em Rust e o frontend do Tauri, integrando as APIs do Linux no diretório `windows/codenotch/src/` e `linux/`:

* **Rust/Tauri:** Core da aplicação de desktop, UI renderizada em WebKitGTK.
* **`platform.rs`:** Abstrai detecção do display server (Wayland vs X11 via variáveis de ambiente), percurso de árvores de processos no Linux (usando `/proc`) através da struct `ProcMaps`, e injeta `GDK_BACKEND=x11` para contornar limitações do GNOME Wayland (Mutter).
* **`cli_discovery.rs`:** Realiza varreduras em caminhos padrão (`/usr/local/bin`, `~/.local/bin`, `~/bin`) e gerenciadores (`nvm`, `volta`, `asdf`, `npm` global) para localizar utilitários como `claude`, `codex` e `agy`.
* **`doctor.rs`:** Provê o módulo ativado por `make linux-doctor` para diagnosticar silenciosamente o ambiente e dependências, sem vazar segredos.
* **Provider Codex (`codex.rs`):** Lê os tokens diretamente de `~/.codex/auth.json` ou faz varredura de fallbacks via arquivos de sessão (sqlite local/rollout).
* **Provider Antigravity (`antigravity.rs`):** Opera localizando o processo ou invocando a CLI (`agy`). Utiliza bridging via endpoint local do `language_server` ou faz parsing dos logs de transcrição de forma agnóstica.
* **XDG:** Utiliza estritamente XDG Base Directories (`~/.config/codenotch/` para arquivos como `ide_watch.json`, `watch_state.json` e `codenotch.pid`).
* **`systemd --user`:** Serviço `codenotch-ide-watch.service` isolado por usuário para auto-execução vinculada às IDEs.
* **`codenotch-hook`:** Binário stand-alone, zero-dependency, isolado do Tauri para reportar eventos e interceptações da CLI para o Codenotch.
* **Tray:** Integração nativa na bandeja via Tauri utilizando backend `libayatana-appindicator3` (`tray.rs`, `trayicon.rs`).
* **Packaging:** Arquivos de empacotamento (`tauri.linux.bundle.conf.json`) devidamente ajustados para gerar `.deb` e `AppImage`.

## 5. Gerenciamento de execução

A infraestrutura autônoma de inicialização vive em `linux/bin/` e é ancorada no Systemd:

* `codenotch-control`: CLI em Python para orquestração manual.
* `codenotch-ide-watch`: Daemon em Python que realiza o polling (zero-CPU/sleep).
* `codenotch-ide-watch.service`: Unidade `systemd --user` que gerencia o daemon.

Comandos expostos pelo `codenotch-control`:
* `start` / `stop` / `restart` / `status` / `toggle`: Controlam a execução imediata do processo principal `codenotch`. O `stop` não apenas finaliza o aplicativo, mas sinaliza ao watcher para ignorar a sessão atual temporariamente.
* `auto-on` / `auto-off` / `auto-status`: Gerenciam o serviço `systemd --user` nativo. `auto-off` desabilita de fato o autorun no login e desliga o serviço.
* `pause` / `resume`: Escrevem no arquivo de estado XDG `watch_state.json` sinalizando para o daemon parar de iniciar o `codenotch` mesmo que IDEs válidas estejam abertas. O serviço continua rodando dormente.
* `setup` / `uninstall`: Comandos internos de scaffolding para instanciar as units do systemd no `~/.config/systemd/user/`.

A autoridade final sobre não rodar é do usuário via `auto-off` ou `pause`.

## 6. IDEs atualmente detectadas

A detecção atual é **restrita e process-based (PID/cmdline)**, feita pelo script Python `codenotch-ide-watch` que lê a configuração padronizada de `~/.config/codenotch/ide_watch.json`.

As únicas 3 aplicações hoje mapeadas e verificadas via nome/processo (`pgrep -x`) são:
1. **Cursor** (executável: `/usr/share/cursor/cursor`, processo: `cursor`)
2. **Antigravity** (executável: `/opt/antigravity/antigravity`, processo: `antigravity`)
3. **VS Code** (executável: `/usr/share/code/code`, processo: `code`)

Essa implementação é simplificada e não avalia o foco da janela ativa (X11 X_Window/Wayland active toplevel). 

## 7. Providers

As quotas são validadas estritamente via métodos offline/read-only sem gravar dados persistentes de auth. 

### Codex
* **Quota descoberta:** Requisição GET para `https://chatgpt.com/backend-api/wham/usage` (quando token está íntegro) ou varredura de fallbacks de arquivos JSON/SQLite de histórico de interações gerados localmente pelo próprio client do Codex.
* **Paths utilizados:** `~/.codex/auth.json` e `~/.codex/sessions/`.
* **Autenticação:** Baseada na extração passiva de `tokens.access_token` e `tokens.account_id` contidos no json local. Não emite requests para login.
* **Limites:** Capaz de ler `rate_limit.primary_window` e plan limits de contas Free e Plus.

### Antigravity
* **Quota descoberta:** Via CLI nativa `agy --print /usage` acionada em pseudo-terminal isolado via Tauri Command. 
* **language_server:** Faz POST num socket efêmero local (ex: `127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary`).
* **Autenticação local:** Intercepta localmente enviando o header `x-codeium-csrf-token` lido dos argumentos do processo e ignora certificação ssl estrita apenas para loopback.
* **Fallbacks (Logs/Cache):** Lê arquivos locais em `~/.gemini/antigravity*/brain/*/.system_generated/logs/transcript.jsonl` contabilizando a quantidade de linhas em que `source=="MODEL"` num mesmo dia.
* **Cotas provadas:** Leituras bem sucedidas documentam acesso a informações em "Weekly Limit Remaining" via language_server e uso total através dos transcripts.

> NENHUM OAuth token, CSRF token, chave ou cookie é gravado, persistido no código, copiado em disco, transmitido remotamente, ou gerado a partir desta base.

## 8. Posicionamento atual da notch

**ATENÇÃO EXTREMA NESTA SEÇÃO: Esta é a raiz dos defeitos visuais no Linux.**

* **Como a janela é posicionada hoje:** Utiliza as APIs internas de posicionamento do Tauri apontadas para coordenadas absolutas da borda do monitor baseadas nas dimensões nativas da tela primária.
* **Uso de GDK e XWayland:** Pelo fato do GNOME (Mutter) no Wayland proibir protocolos de window rules irrestritas (`wlr-layer-shell`), o `platform.rs` força o uso da camada GDK_BACKEND=x11 sob ambientes GNOME Wayland. 
* **Propriedades (Window Hints):** A janela requisita propriedades como `keep_above`, `skip_taskbar`, e evita aceitar o foco (`accept_focus = false`).
* **Comportamento real / Limitações:** 
  * O que foi **implementado e testado:** Fixação nas bordas exatas do monitor sob X11 puro (e via XWayland).
  * O que foi apenas **assumido/desejado:** Assumiu-se que prender a notch ao monitor proveria a mesma experiência que no macOS. O Linux possui fluxos de WM tiling e janelas independentes (IDEs lado a lado), fazendo a Notch flutuar sobre locais mortos. O posicionamento não tem relação topológica com a janela da IDE atualmente. 

## 9. Bugs atuais confirmados

* **BUG A — POSICIONAMENTO:** A notch frequentemente surge flutuando no meio da tela ou presa à borda física do display ao abrir VS Code ou Antigravity num sistema Wayland não redimensionado. A notch **não está acompanhando a janela da IDE**.
* **BUG B — IDE WATCHER INCOMPLETO:** O daemon não monitora as opções requisitadas pelo usuário (Android Studio, ChatGPT Desktop/Codex, IntelliJ IDEA, PyCharm, CLion, WebStorm, Cursor). Assume equivocadamente apenas 3 processos e se baseia apenas no nome do binário no `pgrep`, o que é falho em instalações flatpak ou snap.
* **BUG C — AUTOSTART SEM RESULTADO VISUAL CONFIÁVEL:** Abrir as IDEs testadas inicia a Notch, mas seu posicionamento é absoluto (Bug A), causando desconexão total entre o acionamento (que funciona) e o HUD (que quebra).

## 10. Comportamento desejado para a próxima versão

O próximo ciclo de desenvolvimento **deve** atender a estes requisitos:
* Ter apenas uma única instância do processo Codenotch em execução por usuário.
* Detectar todas as IDEs suportadas com confiabilidade.
* Se nenhuma IDE suportada estiver ativa (mesmo minimizada), fechar/ocultar o processo (usando os callbacks definidos).
* Ao iniciar a primeira IDE validada: iniciar a Notch.
* Ao fechar a última IDE aberta: encerrar a Notch.
* Se n IDEs estiverem abertas, reaproveitar a mesma instância.
* **VINCULAÇÃO ESPACIAL:** A Notch DEVE fixar e ancorar sua geometria na janela do Sistema/IDE suportado que estiver **atualmente em foco**.
* Ao alternar via Alt-Tab entre a IDE X e a IDE Y, a Notch deve saltar e colar nas extremidades do focus target correspondente.
* Ao arrastar, minimizar, expandir, ou migrar a IDE entre monitores, a Notch deve acompanhar os vetores do Host Window.
* Se a IDE for minimizada, a Notch nunca deve permanecer flutuando órfã no desktop.
* Os controles manuais CLI continuam independentes.
* O comando `auto-off` será respeitado como kill-switch supremo do Watcher.

## 11. Testes existentes

A base de testes encontra-se hígida:
* Rust Unit Tests sob `windows/codenotch/src/`.
* Testes pontuais de scripts de gerência na root `linux/`.
* Invocação formal:
  * `make linux-test` (Cobre testes de Rust gerais)
  * `make linux-check` (Validação sintática cargo)
  * `make linux-doctor` (Checklist de ambiente e path probing)
* Resultados: Atualmente os testes passam, pois a falha de Wayland e de foco são semânticas (UI/UX) e não memory-faults.

## 12. Build / execução

O arquivo raiz `Makefile` unifica as tarefas nas rotinas nativas:

```sh
make linux-build       # Compila hook e o frontend (windows/target/release/)
make linux-run         # Executa a build compilada diretamente
make linux-test        # Unit test suite do backend
make linux-check       # Cargo clippy / check format
make linux-doctor      # Ferramenta de scan de path, `/proc` e libs gráficas
make linux-package     # Empacota em `.deb` e `.AppImage` no Tauri bundle dir
```

Artefatos de packaging: 
`windows/target/release/bundle/deb/` e `windows/target/release/bundle/appimage/`.

## 13. Release / README

* A versão preliminar existente é classificada como **Linux Preview 0.1.0**.
* Arquivos binários distribuídos são os formatos gerados pelo workflow `.deb` e `AppImage`.
* As documentações estão plenamente atualizadas no `README.md` da raiz e detalhadas em `linux/README.md`.
* As limitações relacionadas ao GNOME Wayland e XWayland foram expostas explicitamente aos usuários no README.

---

## NEXT TASK

A continuação imediata dos trabalhos deve ser direcionada para as metas:

1. **Ampliar detecção de ambientes de desenvolvimento:** Reescrever/expandir a lista em `codenotch-ide-watch` e `ide_watch.json` para detectar Android Studio, Antigravity, ChatGPT Desktop / Codex, IntelliJ IDEA, PyCharm, CLion, WebStorm, VS Code e Cursor através de mecanismos universais (PIDs e window classes reais).
2. **Substituir posicionamento relativo ao monitor por posicionamento relativo à janela da IDE ativa:** Interceptar dados do X11 (via `xdotool` ou xlib-rs) e Wayland (via shells/extensões onde possível) para capturar a bounding box real do editor ativo e mover a janela webview Tauri dinamicamente para os frames dessa host-window.
3. **Corrigir o comportamento no GNOME Wayland de forma confiável:** Aplicar lógicas robustas de workaround para contornar o floating glitch e lidar com maximizações, mudanças de foco e minimizações em ambientes restritos (XWayland).
4. **Preservar todas as funcionalidades já existentes:** Garantir que detecções em `/proc`, providers Codex/Antigravity, empacotamentos e o controle de estado e CLI do `codenotch-control` não regridam durante a refatoração.
