{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pwndbg = {
      url = "github:pwndbg/pwndbg";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    doomgeneric = {
      url = "github:ozkl/doomgeneric";
      flake = false;
    };
    doom1-wad = {
      url = "https://distro.ibiblio.org/slitaz/sources/packages/d/doom1.wad";
      flake = false;
    };
    dash-src = {
      url = "http://gondor.apana.org.au/~herbert/dash/files/dash-0.5.12.tar.gz";
      flake = false;
    };
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      pwndbg,
      doomgeneric,
      doom1-wad,
      dash-src,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain;
        kani = import ./nix/kani.nix { inherit pkgs; };

        riscv-toolchain = pkgs.pkgsCross.riscv64-musl.pkgsStatic.extend (
          final: prev: {
            musl = prev.musl.overrideAttrs (old: {
              configureFlags = old.configureFlags ++ [
                "--disable-optimize"
              ];
              hardeningDisable = [ "fortify" ];
              separateDebugInfo = false;
              dontStrip = true;
              postPatch = old.postPatch + ''
                mkdir -p $out/src
                cp -r ./ $out/src/
              '';
            });
          }
        );

        musl-riscv = riscv-toolchain.musl;

        basePackages = [
          pkgs.qemu
          pkgs.cargo-nextest
          pkgs.just
          (pkgs.python3.withPackages (ps: [
            ps.pygdbmi
            ps.mcp
          ]))
          rustToolchain
          riscv-toolchain.buildPackages.gcc
          riscv-toolchain.buildPackages.binutils
          kani
        ];

        commonEnv = {
          # Needed for bindgen
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        };

        hook = ''
          rm -rf headers/linux_headers headers/musl_headers

          ln -sf ${musl-riscv.linuxHeaders}/ headers/linux_headers
          ln -sf ${musl-riscv.dev}/include headers/musl_headers

          export DOOMGENERIC_SRC=${doomgeneric}
          export DOOM1_WAD=${doom1-wad}
          export DASH_SRC=${dash-src}

          mkdir -p kernel/compiled_userspace_nix
          just build-dash build-doom

          just mcp-server
        '';

      in
      {
        devShells.default = pkgs.mkShell (
          commonEnv
          // {
            nativeBuildInputs = [
              pkgs.gdb
              pkgs.tmux
              pwndbg.packages.${system}.default
              pkgs.typos-lsp
              pkgs.dtc
              pkgs.e2fsprogs
              pkgs.gnumake
            ]
            ++ basePackages;
            shellHook = hook;
          }
        );

        packages.ci-image =
          let
            tools = pkgs.buildEnv {
              name = "solaya-ci-tools";
              paths = [
                pkgs.bash
                pkgs.coreutils
                pkgs.git
                pkgs.cacert
                pkgs.gnugrep
                pkgs.findutils
                pkgs.gawk
                pkgs.gnused
                pkgs.gnutar
                pkgs.gzip
                pkgs.gnumake
                pkgs.qemu
                pkgs.cargo-nextest
                pkgs.just
                rustToolchain
                kani
                riscv-toolchain.buildPackages.gcc.out
                riscv-toolchain.buildPackages.binutils.out
                pkgs.llvmPackages.libclang.lib
                musl-riscv.dev
                musl-riscv.linuxHeaders
              ];
              ignoreCollisions = true;
            };
            sources = pkgs.runCommand "solaya-ci-sources" { } ''
              mkdir -p $out/opt
              ln -s ${doomgeneric} $out/opt/doomgeneric
              cp ${doom1-wad} $out/opt/doom1.wad
              ln -s ${dash-src} $out/opt/dash-src
            '';
            # GitHub Actions mounts its own Node.js binary which needs glibc + libstdc++
            glibcLinker = pkgs.runCommand "glibc-linker" { } ''
              mkdir -p $out/lib64
              ln -s ${pkgs.glibc}/lib/ld-linux-x86-64.so.2 $out/lib64/ld-linux-x86-64.so.2
              mkdir -p $out/lib
              ln -s ${pkgs.glibc}/lib/* $out/lib/
              ln -s ${pkgs.stdenv.cc.cc.lib}/lib/libstdc++.so* $out/lib/
            '';
          in
          pkgs.dockerTools.buildLayeredImage {
            name = "ghcr.io/sysheap/solaya-ci";
            tag = "latest";
            contents = [
              tools
              sources
              glibcLinker
            ];
            config = {
              Env = [
                "LIBCLANG_PATH=${pkgs.llvmPackages.libclang.lib}/lib"
                "LINUX_HEADERS_PATH=${musl-riscv.linuxHeaders}"
                "MUSL_HEADERS_PATH=${musl-riscv.dev}/include"
                "DOOMGENERIC_SRC=/opt/doomgeneric"
                "DOOM1_WAD=/opt/doom1.wad"
                "DASH_SRC=/opt/dash-src"
                "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              ];
              Cmd = [ "${pkgs.bash}/bin/bash" ];
            };
          };
      }
    );
}
