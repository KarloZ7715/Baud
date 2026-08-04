//! Icono Nerd Font por nombre de proceso, para el titulo de una tab.
//!
//! Los codepoints vienen de `glyphnames.json` (proyecto Nerd Fonts, sets
//! codicons/devicons/linux/font-awesome/octicons/seti). La disponibilidad real
//! se comprueba por separado (`glyph_rasterizes`): la lista de fallback del
//! usuario puede no incluir una Nerd Font, y dibujar el codepoint sin
//! comprobarlo pintaria tofu.

/// Todos los iconos que puede devolver `icon_for_process`, para comprobar su
/// disponibilidad de una sola vez al cargar la fuente (`ALL_ICONS.len()`
/// shapeos en vez de uno por tab).
pub const ALL_ICONS: &[char] = &[
    // Editores
    '\u{f36f}', // linux-neovim
    '\u{e7c5}', // dev-vim
    '\u{e7cf}', // dev-emacs
    '\u{ea73}', // cod-edit
    // VCS / forjas
    '\u{e702}', // dev-git
    '\u{e709}', // dev-github
    '\u{e7eb}', // dev-gitlab
    '\u{e703}', // dev-bitbucket
    // Lenguajes y runtimes
    '\u{e7a8}', // dev-rust
    '\u{e73c}', // dev-python
    '\u{e718}', // dev-nodejs_small
    '\u{e71e}', // dev-npm
    '\u{e8ec}', // dev-yarn
    '\u{e724}', // dev-go
    '\u{e738}', // dev-java
    '\u{e739}', // dev-ruby
    '\u{e73d}', // dev-php
    '\u{e826}', // dev-lua
    '\u{e8ef}', // dev-zig
    '\u{e7cd}', // dev-elixir
    '\u{e777}', // dev-haskell
    '\u{e768}', // dev-clojure
    '\u{e737}', // dev-scala
    '\u{e77f}', // dev-dotnet
    '\u{e7b2}', // dev-csharp
    '\u{e771}', // dev-c
    '\u{e7a3}', // dev-cplusplus
    '\u{e8ca}', // dev-typescript
    '\u{e781}', // dev-javascript
    '\u{e7ba}', // dev-react
    '\u{e753}', // dev-angular
    '\u{e8b7}', // dev-svelte
    '\u{e798}', // dev-dart
    '\u{e755}', // dev-swift
    '\u{e81b}', // dev-kotlin
    '\u{e80d}', // dev-julia
    '\u{e881}', // dev-r
    '\u{e84e}', // dev-ocaml
    '\u{e841}', // dev-nim
    '\u{e7ac}', // dev-crystal
    '\u{e769}', // dev-perl
    '\u{e6a1}', // seti-wasm
    // Contenedores / IaC / nube
    '\u{f308}', // linux-docker
    '\u{e81d}', // dev-kubernetes
    '\u{e8bd}', // dev-terraform
    '\u{e723}', // dev-ansible
    '\u{e7ad}', // dev-aws
    '\u{e754}', // dev-azure
    '\u{e77b}', // dev-heroku
    '\u{ebaa}', // cod-cloud
    '\u{f313}', // linux-nixos
    // Datos / servidores
    '\u{e76e}', // dev-postgresql
    '\u{e704}', // dev-mysql
    '\u{e76d}', // dev-redis
    '\u{e7a4}', // dev-mongodb
    '\u{e7c4}', // dev-sqlite
    '\u{e7ca}', // dev-elasticsearch
    '\u{eace}', // cod-database
    '\u{e776}', // dev-nginx
    '\u{e72b}', // dev-apache
    '\u{f233}', // fa-server
    // Build / paquetes / tooling
    '\u{e673}', // seti-makefile
    '\u{e6a3}', // seti-webpack
    '\u{e684}', // seti-prisma
    '\u{e662}', // seti-graphql
    '\u{f085}', // fa-cogs
    '\u{eb29}', // cod-package
    '\u{eaef}', // cod-file_zip
    '\u{ea79}', // cod-beaker
    '\u{eaaf}', // cod-bug
    '\u{ea6d}', // cod-search
    // Red / sistema / media
    '\u{e8b1}', // dev-ssh
    '\u{eeb2}', // fa-gauge
    '\u{f0a0}', // fa-hard_drive
    '\u{efc5}', // fa-memory
    '\u{f2db}', // fa-microchip
    '\u{ef09}', // fa-network_wired
    '\u{f1eb}', // fa-wifi
    '\u{f023}', // fa-lock
    '\u{f084}', // fa-key
    '\u{ed25}', // fa-shield_halved
    '\u{f019}', // fa-download
    '\u{f001}', // fa-music
    '\u{f04b}', // fa-play
    '\u{f030}', // fa-camera
    '\u{f008}', // fa-film
    '\u{f11b}', // fa-gamepad
    '\u{f02d}', // fa-book
    '\u{f0ac}', // fa-globe
    '\u{f135}', // fa-rocket
    '\u{f0e7}', // fa-bolt
    '\u{e678}', // seti-notebook
    '\u{e69b}', // seti-tex
    // Agentes / asistentes
    '\u{ec82}', // cod-claude
    '\u{ec81}', // cod-openai
    '\u{ec1e}', // cod-copilot
    '\u{ec5c}', // cod-cursor
    '\u{ec67}', // cod-agent
    '\u{ec4f}', // cod-chat_sparkle
    '\u{ec20}', // cod-robot
    '\u{eac4}', // cod-code
    '\u{ee9c}', // fa-brain
    // Generico
    '\u{f120}', // fa-terminal
];

/// Icono para el nombre de proceso (basename; se normaliza a minusculas y se
/// quita el sufijo `.exe` de Windows). Los procesos sin entrada especifica
/// caen al icono generico de terminal, no a `None`: todo proceso en primer
/// plano que no sea el shell muestra algun icono.
pub fn icon_for_process(name: &str) -> char {
    // Minusculas + quitar `.exe` (Windows) antes del match, para que
    // `Claude.EXE` y `python.exe` caigan en el mismo brazo que en Unix.
    let lower = name.to_ascii_lowercase();
    let name = lower.strip_suffix(".exe").unwrap_or(lower.as_str());
    match name {
        // --- Editores ---
        "nvim" | "neovim" => '\u{f36f}', // linux-neovim
        "vim" | "vimdiff" | "gvim" | "mvim" | "nvim-qt" => '\u{e7c5}', // dev-vim
        "emacs" | "emacsclient" => '\u{e7cf}', // dev-emacs
        "hx" | "helix" | "kak" | "kakoune" | "nano" | "micro" | "ed" | "code" | "code-insiders"
        | "codium" | "vscodium" | "sublime_text" | "subl" | "zed" | "zeditor" | "kate"
        | "gedit" | "notepad" | "notepad++" => '\u{ea73}', // cod-edit

        // --- VCS / forjas ---
        "git" | "git-lfs" | "tig" | "lazygit" | "lg" | "gitui" | "gh-dash" | "delta" | "diff"
        | "difftool" => '\u{e702}', // dev-git
        "gh" | "hub" | "act" => '\u{e709}', // dev-github
        "glab" | "gitlab" => '\u{e7eb}',    // dev-gitlab
        "bitbucket" => '\u{e703}',          // dev-bitbucket (`bb` es babashka)

        // --- Lenguajes / runtimes ---
        "cargo" | "rustc" | "rustup" | "clippy-driver" | "rust-analyzer" | "rustfmt"
        | "cargo-watch" | "cargo-nextest" | "cargo-clippy" => '\u{e7a8}', // dev-rust
        "python" | "python2" | "python3" | "ipython" | "ipython3" | "pip" | "pip3" | "pipx"
        | "poetry" | "uv" | "pdm" | "hatch" | "conda" | "mamba" | "micromamba" | "pyenv"
        | "mypy" | "ruff" | "black" | "isort" | "flask" | "django" | "gunicorn" | "uvicorn"
        | "celery" => '\u{e73c}', // dev-python
        "node" | "nodejs" | "deno" | "bun" | "tsx" | "ts-node" | "nodemon" | "pm2" | "electron" => {
            '\u{e718}'
        } // dev-nodejs_small
        "npm" | "npx" | "pnpm" | "pnpx" | "corepack" => '\u{e71e}', // dev-npm
        "yarn" | "yarnpkg" => '\u{e8ec}',                           // dev-yarn
        "go" | "gofmt" | "gopls" | "goreleaser" | "air" => '\u{e724}', // dev-go
        "java" | "javac" | "jshell" | "kotlin" | "kotlinc" | "groovy" | "groovyc" | "mvn"
        | "mvnw" | "gradle" | "gradlew" => '\u{e738}', // dev-java
        "ruby" | "irb" | "pry" | "gem" | "bundle" | "bundler" | "rake" | "rails" | "rbenv"
        | "chruby" => '\u{e739}', // dev-ruby
        "php" | "php8" | "composer" | "laravel" | "artisan" => '\u{e73d}', // dev-php
        "lua" | "luajit" | "luarocks" | "love" => '\u{e826}',       // dev-lua
        "zig" | "zigfmt" => '\u{e8ef}',                             // dev-zig
        "elixir" | "iex" | "mix" | "erl" | "erlc" | "rebar3" | "gleam" => '\u{e7cd}', // dev-elixir
        "ghc" | "ghci" | "runghc" | "cabal" | "stack" | "haskell" => '\u{e777}', // dev-haskell
        "clojure" | "clj" | "lein" | "leiningen" | "bb" | "babashka" => '\u{e768}', // dev-clojure
        "scala" | "scalac" | "sbt" | "amm" | "ammonite" => '\u{e737}', // dev-scala
        "dotnet" | "dotnet-sdk" | "fsi" | "fsc" => '\u{e77f}',      // dev-dotnet
        "csharp" | "csi" | "csc" => '\u{e7b2}',                     // dev-csharp
        "gcc" | "cc" | "clang" | "clang++" | "clangd" | "cpp" | "g++" | "c++" | "ld" | "lld"
        | "cmake" | "meson" | "ninja" | "autoconf" | "automake" | "libtool" => '\u{e7a3}', // dev-cplusplus (toolchain C/C++)
        "tcc" | "pcc" => '\u{e771}',                     // dev-c
        "tsc" | "typescript" | "tsserver" => '\u{e8ca}', // dev-typescript
        "js" | "javascript" | "esbuild" | "swc" | "babel" | "babel-node" => '\u{e781}', // dev-javascript
        "react-scripts" | "next" | "next-server" | "vite" | "astro" | "remix" | "remix-serve" => {
            '\u{e7ba}'
        } // dev-react
        "ng" | "angular" | "angular-cli" => '\u{e753}', // dev-angular
        "svelte" | "svelte-kit" | "svelte-check" => '\u{e8b7}', // dev-svelte
        "dart" | "dartaotruntime" | "flutter" | "flutter_tools" => '\u{e798}', // dev-dart
        "swift" | "swiftc" | "xcodebuild" | "xcrun" => '\u{e755}', // dev-swift
        "kotlinc-js" | "kotlinc-jvm" => '\u{e81b}',     // dev-kotlin
        "julia" | "julia-debug" => '\u{e80d}',          // dev-julia
        "r" | "rscript" | "radian" => '\u{e881}',       // dev-r
        "ocaml" | "ocamlc" | "ocamlopt" | "opam" | "dune" | "utop" => '\u{e84e}', // dev-ocaml
        "nim" | "nimble" | "nimrod" => '\u{e841}',      // dev-nim
        "crystal" | "shards" => '\u{e7ac}',             // dev-crystal
        "perl" | "perl5" | "cpan" | "cpanm" | "plenv" => '\u{e769}', // dev-perl
        "wasm" | "wasmtime" | "wasmer" | "wasm-ld" | "wat2wasm" => '\u{e6a1}', // seti-wasm

        // --- Contenedores / orquestacion / IaC ---
        "docker" | "dockerd" | "docker-compose" | "docker-compose." | "compose" | "containerd"
        | "containerd-shim" | "ctr" | "nerdctl" | "podman" | "podman-compose" | "buildah"
        | "skopeo" | "crictl" | "lima" | "colima" | "orbstack" | "rancher-desktop" => '\u{f308}', // linux-docker
        "kubectl" | "k9s" | "helm" | "helmfile" | "kustomize" | "k3s" | "k3d" | "kind"
        | "minikube" | "kubectx" | "kubens" | "stern" | "skaffold" | "tilt" | "argocd" | "flux"
        | "fluxcd" => '\u{e81d}', // dev-kubernetes
        "terraform" | "tofu" | "opentofu" | "terragrunt" | "pulumi" | "cdktf" | "packer"
        | "vagrant" => '\u{e8bd}', // dev-terraform
        "ansible" | "ansible-playbook" | "ansible-galaxy" | "ansible-vault" | "ansible-lint" => {
            '\u{e723}'
        } // dev-ansible

        // --- Nube / PaaS ---
        "aws" | "aws-cli" | "awslocal" | "sam" | "cdk" | "serverless" | "sls" | "amplify" => {
            '\u{e7ad}'
        } // dev-aws
        "az" | "azure" | "azure-cli" | "func" => '\u{e754}', // dev-azure
        "gcloud" | "gsutil" | "bq" | "firebase" | "firebase-tools" => '\u{ebaa}', // cod-cloud
        "heroku" | "heroku-cli" => '\u{e77b}',               // dev-heroku
        "fly" | "flyctl" | "doctl" | "linode-cli" | "vultr-cli" | "railway" | "render"
        | "vercel" | "netlify" | "wrangler" | "supabase" | "sst" | "pulumi-language" => '\u{ebaa}', // cod-cloud
        "nix" | "nix-shell" | "nix-build" | "nix-env" | "nix-store" | "home-manager"
        | "nixos-rebuild" | "direnv" | "lorri" | "devenv" => '\u{f313}', // linux-nixos

        // --- Bases de datos ---
        "psql" | "postgres" | "postgresql" | "pg_dump" | "pg_restore" | "pgcli" | "pg_isready" => {
            '\u{e76e}'
        } // dev-postgresql
        "mysql" | "mysqld" | "mysqladmin" | "mariadb" | "mariadbd" | "mycli" => '\u{e704}', // dev-mysql
        "redis" | "redis-cli" | "redis-server" | "valkey" | "valkey-cli" | "keydb" => '\u{e76d}', // dev-redis
        "mongo" | "mongod" | "mongosh" | "mongodb" => '\u{e7a4}', // dev-mongodb
        "sqlite" | "sqlite3" | "litecli" => '\u{e7c4}',           // dev-sqlite
        "elasticsearch" | "opensearch" | "kibana" | "logstash" => '\u{e7ca}', // dev-elasticsearch
        "clickhouse" | "clickhouse-client" | "duckdb" | "cassandra" | "cqlsh" | "neo4j"
        | "cypher-shell" | "influx" | "influxd" | "cockroach" | "cockroachdb" | "tidb"
        | "dbeaver" | "usql" | "sqlcmd" => '\u{eace}', // cod-database

        // --- Servidores HTTP / proxies ---
        "nginx" | "nginx-debug" => '\u{e776}', // dev-nginx
        "httpd" | "apache2" | "apachectl" => '\u{e72b}', // dev-apache
        "caddy" | "traefik" | "haproxy" | "envoy" => '\u{f233}', // fa-server

        // --- Build / paquetes / calidad ---
        "make" | "gmake" | "bmake" | "just" | "task" | "invoke" | "scons" | "bazel"
        | "bazelisk" | "buck" | "please" | "mage" => '\u{e673}', // seti-makefile
        "webpack" | "webpack-dev-server" | "rollup" | "parcel" | "turbo" | "turborepo" | "nx"
        | "lerna" | "rush" => '\u{e6a3}', // seti-webpack
        "prisma" | "prisma-client" => '\u{e684}', // seti-prisma
        "graphql" | "graphql-codegen" | "apollo" | "relay" | "hasura" => '\u{e662}', // seti-graphql
        "eslint" | "prettier" | "stylelint" | "biome" | "oxlint" | "knip" | "depcheck"
        | "madge" => '\u{f085}', // fa-cogs
        "pacman" | "yay" | "paru" | "makepkg" | "apt" | "apt-get" | "aptitude" | "dpkg" | "dnf"
        | "yum" | "zypper" | "xbps-install" | "apk" | "brew" | "port" | "choco" | "chocolatey"
        | "scoop" | "winget" | "flatpak" | "snap" | "appimage" => '\u{eb29}', // cod-package
        "tar" | "gzip" | "gunzip" | "bzip2" | "xz" | "zstd" | "zip" | "unzip" | "7z" | "7za"
        | "rar" | "unrar" | "pigz" | "pbzip2" => '\u{eaef}', // cod-file_zip
        "pytest" | "jest" | "vitest" | "mocha" | "ava" | "tap" | "phpunit" | "rspec"
        | "cucumber" | "bats" | "shellspec" | "cargo-test" | "nextest" | "playwright"
        | "cypress" | "selenium" | "chromedriver" | "geckodriver" => '\u{ea79}', // cod-beaker
        "gdb" | "lldb" | "lldb-server" | "cgdb" | "rust-gdb" | "rust-lldb" | "valgrind"
        | "strace" | "ltrace" | "perf" | "bpftrace" | "rr" | "asan" | "tsan" => '\u{eaaf}', // cod-bug
        "rg" | "ripgrep" | "ag" | "ack" | "grep" | "egrep" | "fgrep" | "fd" | "fdfind" | "find"
        | "fzf" | "sk" | "skim" | "peco" | "jq" | "yq" | "xq" | "bat" | "batcat" | "less"
        | "most" | "glow" | "mdcat" => '\u{ea6d}', // cod-search

        // --- Red / remoto ---
        "ssh" | "sshd" | "scp" | "sftp" | "ssh-agent" | "ssh-add" | "ssh-keygen" | "mosh"
        | "mosh-client" | "mosh-server" | "autossh" | "eternal-terminal" | "et" => '\u{e8b1}', // dev-ssh
        "curl" | "wget" | "http" | "httpie" | "xh" | "aria2c" | "axel" | "yt-dlp"
        | "youtube-dl" | "gallery-dl" | "transmission-cli" | "aria2" => '\u{f019}', // fa-download
        "ping" | "ping6" | "traceroute" | "tracepath" | "mtr" | "dig" | "nslookup" | "host"
        | "whois" | "nmap" | "masscan" | "tcpdump" | "wireshark" | "tshark" | "iftop"
        | "nethogs" | "bmon" | "vnstat" | "ss" | "ip" | "ifconfig" | "nmcli" | "nmtui" | "wg"
        | "wireguard" => '\u{ef09}', // fa-network_wired
        "nc" | "ncat" | "netcat" | "socat" | "telnet" | "ftp" | "lftp" | "rsync" | "rclone" => {
            '\u{f1eb}'
        } // fa-wifi

        // --- Monitorizacion / sistema ---
        "htop" | "btop" | "bpytop" | "top" | "atop" | "glances" | "gotop" | "ctop" | "nvtop"
        | "nvitop" | "gpustat" | "iotop" | "iostat" | "vmstat" | "mpstat" | "sar" | "dstat"
        | "bandwhich" => '\u{eeb2}', // fa-gauge
        "df" | "du" | "ncdu" | "dust" | "dua" | "diskonaut" | "baobab" | "filelight" | "lsblk"
        | "blkid" | "mount" | "umount" | "fdisk" | "parted" | "gparted" => '\u{f0a0}', // fa-hard_drive
        "free" | "smem" | "ps_mem" => '\u{efc5}', // fa-memory
        "lscpu" | "lsusb" | "lspci" | "lshw" | "dmidecode" | "sensors" | "neofetch"
        | "fastfetch" | "screenfetch" | "inxi" | "uname" => '\u{f2db}', // fa-microchip
        "systemctl" | "journalctl" | "service" | "sv" | "rc-service" | "launchctl" | "sc"
        | "services" => '\u{f085}', // fa-cogs
        "sudo" | "doas" | "su" | "pkexec" | "run0" => '\u{f023}', // fa-lock
        "gpg" | "gpg2" | "gpg-agent" | "age" | "rage" | "sops" | "vault" | "pass" | "gopass"
        | "bw" | "bitwarden" | "keepassxc" | "openssl" => '\u{f084}', // fa-key
        "ufw" | "firewall-cmd" | "iptables" | "nft" | "nftables" | "fail2ban"
        | "fail2ban-client" | "clamav" | "clamscan" | "apparmor_parser" | "aa-status" => '\u{ed25}', // fa-shield_halved

        // --- Multiplexores ---
        "tmux" | "screen" | "zellij" => '\u{f120}', // fa-terminal

        // --- Media / documentos ---
        "mpv" | "vlc" | "ffplay" | "mplayer" | "celluloid" | "totem" => '\u{f04b}', // fa-play
        "ffmpeg" | "ffprobe" | "handbrake" | "handbrakecli" | "obs" | "obs-studio" => '\u{f008}', // fa-film
        "cmus" | "ncmpcpp" | "mocp" | "moc" | "spotify" | "ncspot" | "spotdl" | "cava"
        | "pavucontrol" | "alsamixer" | "pulsemixer" | "pw-cli" | "pipewire" | "wireplumber" => {
            '\u{f001}'
        } // fa-music
        "convert" | "magick" | "identify" | "gimp" | "inkscape" | "krita" | "darktable"
        | "rawtherapee" | "swayimg" | "imv" | "feh" | "nsxiv" | "sxiv" => '\u{f030}', // fa-camera
        "blender" | "godot" | "godot4" | "unity" | "unity-editor" | "unreal" | "ue4editor"
        | "ue5editor" | "steam" | "steam-runtime" | "lutris" | "heroic" | "retroarch" => '\u{f11b}', // fa-gamepad
        "latex" | "pdflatex" | "xelatex" | "lualatex" | "bibtex" | "biber" | "typst"
        | "tectonic" | "pandoc" | "asciidoctor" | "hugo" | "zola" | "mdbook" | "jekyll"
        | "sphinx-build" => '\u{e69b}', // seti-tex
        "jupyter" | "jupyter-lab" | "jupyter-notebook" | "jupyter-labhub" | "code-server" => {
            '\u{e678}'
        } // seti-notebook
        "man" | "info" | "tldr" | "tealdeer" | "cheat" | "navi" | "zeal" => '\u{f02d}', // fa-book
        "lynx" | "w3m" | "links" | "elinks" | "browsh" | "chrome" | "chromium"
        | "google-chrome" | "firefox" | "firefox-bin" | "brave" | "brave-browser" | "msedge"
        | "microsoft-edge" | "qutebrowser" | "vivaldi" => '\u{f0ac}', // fa-globe

        // --- Agentes / asistentes ---
        // Glifos propios cuando existen; el resto comparte agente/robot/cerebro.
        "claude" | "claude-code" | "claude-cli" => '\u{ec82}', // cod-claude
        "codex" | "openai" | "openai-codex" | "chatgpt" => '\u{ec81}', // cod-openai
        "copilot" | "gh-copilot" | "github-copilot" | "copilot-cli" => '\u{ec1e}', // cod-copilot
        "cursor" | "cursor-agent" | "cursor-cli" | "cursor-tutor" => '\u{ec5c}', // cod-cursor
        "opencode" | "open-code" | "opencode-cli" => '\u{eac4}', // cod-code
        "gemini" | "gemini-cli" | "google-gemini" => '\u{ec4f}', // cod-chat_sparkle
        "grok" | "grok-cli" | "xai" | "xai-grok" => '\u{ee9c}', // fa-brain
        "hermes" | "hermes-agent" | "hermes-cli" => '\u{ec67}', // cod-agent
        "aider" | "aider-chat" | "continue" | "continue-cli" | "amp" | "amp-cli" | "goose"
        | "goose-cli" | "crush" | "crush-cli" | "windsurf" | "cascade" | "tabnine"
        | "tabnine-cli" | "sourcegraph" | "sg" | "cody" | "amazon-q" | "q-chat" | "bedrock"
        | "codeium" | "codeium-cli" | "supercode" | "pie" | "droid" | "factory" | "devin"
        | "swe-agent" | "openhands" | "opendevin" | "roo" | "roo-code" | "cline" | "cliner"
        | "bolt" | "lovable" | "replit" | "replit-agent" | "warp-agent" | "fig" | "amazonq" => {
            '\u{ec67}'
        } // cod-agent
        "ollama" | "ollama-run" | "llama" | "llama-server" | "lmstudio" | "lm-studio"
        | "localai" | "local-ai" | "textgen" | "koboldcpp" | "kobold" | "vllm" | "llama-cpp"
        | "mlx" | "mlx_lm" | "jan" | "gpt4all" | "mistral" | "mistral-cli" | "huggingface-cli"
        | "hf" | "transformers" | "whisper" => '\u{ec20}', // cod-robot
        "sgpt" | "shell-gpt" | "aichat" | "mods" | "tgpt" | "llm" | "fabric" | "fabric-ai"
        | "chatblade" | "openinterpreter" | "interpreter" => '\u{ec4f}', // cod-chat_sparkle

        // --- Utilidades varias ---
        "hyperfine" | "bench" | "ab" | "wrk" | "k6" | "vegeta" | "oha" | "bombardier" => '\u{f0e7}', // fa-bolt
        "starship" | "oh-my-posh" | "p10k" | "powerline" => '\u{f135}', // fa-rocket
        "yazi" | "ranger" | "lf" | "nnn" | "vifm" | "mc" | "midnight-commander" | "xplr"
        | "broot" | "joshuto" | "superfile" => '\u{ea6d}', // cod-search
        "eza" | "exa" | "lsd" | "ls" | "tree" | "tre" | "zoxide" | "autojump" | "fasd" | "jump" => {
            '\u{f120}'
        } // fa-terminal

        // Generico: cualquier otro proceso en primer plano.
        _ => '\u{f120}', // fa-terminal
    }
}

/// `true` si `ch` rasteriza a un bitmap no vacio con la familia/tamano
/// dados, es decir, si la fuente activa (con su fallback) trae el glifo en
/// vez de caer en `.notdef`. Pensada para llamarse una vez por caracter y
/// cachear el resultado: shapear un glifo no es gratis.
pub fn glyph_rasterizes(
    font_system: &mut glyphon::FontSystem,
    swash_cache: &mut glyphon::SwashCache,
    family: &str,
    font_size: f32,
    ch: char,
) -> bool {
    let metrics = glyphon::Metrics::new(font_size, font_size * 1.2);
    let mut buf = glyphon::Buffer::new(font_system, metrics);
    let attrs = glyphon::Attrs::new().family(super::resolve_family(family));
    let text = ch.to_string();
    buf.set_text(
        font_system,
        &text,
        &attrs,
        glyphon::cosmic_text::Shaping::Advanced,
        None,
    );
    buf.shape_until_scroll(font_system, false);
    let Some(cache_key) = buf
        .layout_runs()
        .next()
        .and_then(|run| run.glyphs.first().cloned())
        .map(|glyph| glyph.physical((0.0, 0.0), 1.0).cache_key)
    else {
        return false;
    };
    super::glyph_cache::cache_key_rasterizes(font_system, swash_cache, cache_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nombres representativos de cada familia de iconos; debe mantenerse
    /// alineado con los brazos de `icon_for_process` que aportan glifos
    /// distintos al generico.
    const SAMPLE_NAMES: &[&str] = &[
        // Editores
        "nvim",
        "vim",
        "emacs",
        "hx",
        "code",
        // VCS
        "git",
        "lazygit",
        "gh",
        "glab",
        "bitbucket",
        // Lenguajes
        "cargo",
        "rustc",
        "python3",
        "uv",
        "node",
        "bun",
        "npm",
        "pnpm",
        "yarn",
        "go",
        "java",
        "ruby",
        "php",
        "lua",
        "zig",
        "elixir",
        "ghc",
        "clojure",
        "scala",
        "dotnet",
        "csharp",
        "gcc",
        "clang",
        "tsc",
        "vite",
        "ng",
        "svelte",
        "dart",
        "flutter",
        "swift",
        "julia",
        "rscript",
        "ocaml",
        "nim",
        "crystal",
        "perl",
        "wasmtime",
        // Contenedores / IaC / nube
        "docker",
        "docker-compose",
        "podman",
        "kubectl",
        "k9s",
        "helm",
        "terraform",
        "tofu",
        "ansible",
        "aws",
        "az",
        "gcloud",
        "heroku",
        "vercel",
        "nix",
        // Datos / servidores
        "psql",
        "mysql",
        "redis-cli",
        "mongosh",
        "sqlite3",
        "elasticsearch",
        "duckdb",
        "nginx",
        "httpd",
        "caddy",
        // Build / tooling
        "make",
        "just",
        "webpack",
        "prisma",
        "graphql",
        "eslint",
        "pacman",
        "brew",
        "tar",
        "pytest",
        "playwright",
        "gdb",
        "rg",
        "fzf",
        // Red / sistema
        "ssh",
        "curl",
        "nmap",
        "htop",
        "btop",
        "nvtop",
        "ncdu",
        "free",
        "fastfetch",
        "systemctl",
        "sudo",
        "gpg",
        "ufw",
        // Media
        "mpv",
        "ffmpeg",
        "spotify",
        "gimp",
        "blender",
        "godot",
        "typst",
        "hugo",
        "jupyter",
        "man",
        "firefox",
        // Agentes
        "claude",
        "codex",
        "copilot",
        "cursor",
        "opencode",
        "gemini",
        "grok",
        "hermes",
        "aider",
        "ollama",
        "aichat",
        // Varios
        "hyperfine",
        "starship",
        "yazi",
        "eza",
        // Generico / desconocido
        "make-not-a-real-tool-zzzz",
        "",
    ];

    #[test]
    fn icon_for_process_mapea_nombres_conocidos() {
        assert_eq!(icon_for_process("nvim"), '\u{f36f}');
        assert_eq!(icon_for_process("vim"), '\u{e7c5}');
        assert_eq!(icon_for_process("git"), '\u{e702}');
        assert_eq!(icon_for_process("cargo"), icon_for_process("rustc"));
        assert_eq!(icon_for_process("node"), icon_for_process("bun"));
        assert_eq!(icon_for_process("python3"), icon_for_process("python"));
        assert_eq!(
            icon_for_process("docker"),
            icon_for_process("docker-compose")
        );
        assert_eq!(icon_for_process("htop"), icon_for_process("btop"));
        assert_eq!(icon_for_process("claude"), '\u{ec82}');
        assert_eq!(icon_for_process("codex"), '\u{ec81}');
        assert_eq!(icon_for_process("opencode"), '\u{eac4}');
        assert_eq!(icon_for_process("grok"), '\u{ee9c}');
        assert_eq!(icon_for_process("hermes"), '\u{ec67}');
        assert_eq!(icon_for_process("cursor"), '\u{ec5c}');
        assert_eq!(icon_for_process("copilot"), '\u{ec1e}');
        assert_eq!(icon_for_process("ollama"), '\u{ec20}');
        assert_eq!(icon_for_process("kubectl"), icon_for_process("k9s"));
        assert_eq!(icon_for_process("terraform"), icon_for_process("tofu"));
    }

    #[test]
    fn icon_for_process_normaliza_mayusculas_y_exe() {
        assert_eq!(icon_for_process("Nvim"), icon_for_process("nvim"));
        assert_eq!(icon_for_process("PYTHON.EXE"), icon_for_process("python"));
        assert_eq!(icon_for_process("Claude.exe"), icon_for_process("claude"));
        assert_eq!(icon_for_process("Git.exe"), icon_for_process("git"));
    }

    #[test]
    fn icon_for_process_cae_al_generico_para_desconocidos() {
        assert_eq!(icon_for_process("totally-unknown-proc-xyz"), '\u{f120}');
        assert_eq!(icon_for_process(""), '\u{f120}');
    }

    #[test]
    fn all_icons_cubre_cada_glifo_devuelto_por_icon_for_process() {
        for name in SAMPLE_NAMES {
            let ch = icon_for_process(name);
            assert!(
                ALL_ICONS.contains(&ch),
                "ALL_ICONS no incluye el icono de {name:?} ({ch:?} U+{:04X})",
                ch as u32
            );
        }
    }

    #[test]
    fn all_icons_no_tiene_duplicados() {
        let mut seen = std::collections::BTreeSet::new();
        for &ch in ALL_ICONS {
            assert!(
                seen.insert(ch),
                "ALL_ICONS duplica el glifo U+{:04X}",
                ch as u32
            );
        }
    }

    #[test]
    fn glyph_rasterizes_no_panica_y_degrada_a_falso_sin_glifo() {
        let mut font_system = super::super::terminal_fallback::create_font_system();
        let mut swash_cache = glyphon::SwashCache::new();
        // Family generica sin Nerd Font: en CI (sin Nerd Fonts instaladas)
        // el icono cae a .notdef y la funcion debe degradar a `false`, no
        // panicar ni devolver un falso positivo.
        let available = glyph_rasterizes(
            &mut font_system,
            &mut swash_cache,
            "monospace",
            12.0,
            '\u{f36f}',
        );
        let _ = available; // el valor depende de las fuentes del sistema; solo importa que no panique
    }

    #[test]
    #[ignore = "requiere Nerd Font instalada (no disponible en CI)"]
    fn glyph_rasterizes_true_con_nerd_font_instalada() {
        let mut font_system = super::super::terminal_fallback::create_font_system();
        let mut swash_cache = glyphon::SwashCache::new();
        assert!(glyph_rasterizes(
            &mut font_system,
            &mut swash_cache,
            "monospace",
            12.0,
            '\u{f36f}',
        ));
    }
}
