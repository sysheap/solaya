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
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      pwndbg,
      doomgeneric,
      doom1-wad,
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

        dash = riscv-toolchain.dash.overrideAttrs (old: {
          hardeningDisable = [ "fortify" ];
          separateDebugInfo = false;
          dontStrip = true;
        });

        doom-riscv = pkgs.stdenv.mkDerivation {
          name = "doom-riscv";
          src = doomgeneric;
          nativeBuildInputs = [
            riscv-toolchain.buildPackages.gcc
            riscv-toolchain.buildPackages.binutils
          ];
          buildPhase = ''
            cd doomgeneric
            cp ${./userspace/doom/dg_solaya.c} dg_solaya.c
            cp ${./userspace/doom/i_video_solaya.c} i_video.c

            # Embed doom1.wad as a binary object in a separate directory
            cp ${doom1-wad} doom1.wad
            mkdir -p wad_obj
            riscv64-unknown-linux-musl-ld -r -b binary -o wad_obj/doom1_wad.o doom1.wad

            CC=riscv64-unknown-linux-musl-gcc
            CFLAGS="-static -O3 -DNORMALUNIX -DLINUX -D_DEFAULT_SOURCE -I."

            SRCS="
              dummy.c am_map.c doomdef.c doomstat.c dstrings.c
              d_event.c d_items.c d_iwad.c d_loop.c d_main.c d_mode.c d_net.c
              f_finale.c f_wipe.c g_game.c hu_lib.c hu_stuff.c info.c
              i_cdmus.c i_endoom.c i_joystick.c i_scale.c i_sound.c i_system.c
              i_timer.c memio.c m_argv.c m_bbox.c m_cheat.c m_config.c
              m_controls.c m_fixed.c m_menu.c m_misc.c m_random.c
              p_ceilng.c p_doors.c p_enemy.c p_floor.c p_inter.c p_lights.c
              p_map.c p_maputl.c p_mobj.c p_plats.c p_pspr.c p_saveg.c
              p_setup.c p_sight.c p_spec.c p_switch.c p_telept.c p_tick.c
              p_user.c r_bsp.c r_data.c r_draw.c r_main.c r_plane.c r_segs.c
              r_sky.c r_things.c sha1.c sounds.c statdump.c st_lib.c st_stuff.c
              s_sound.c tables.c v_video.c wi_stuff.c w_checksum.c w_file.c
              w_main.c w_wad.c z_zone.c w_file_stdc.c i_input.c i_video.c
              mus2mid.c doomgeneric.c dg_solaya.c
            "

            for f in $SRCS; do
              echo "CC $f"
              $CC $CFLAGS -c $f -o ''${f%.c}.o
            done

            $CC $CFLAGS -o doom *.o wad_obj/doom1_wad.o -lm
          '';
          installPhase = ''
            mkdir -p $out/bin
            cp doom $out/bin/doom
          '';
        };

        ciPackages = [
          pkgs.qemu
          pkgs.cargo-nextest
          pkgs.just
          rustToolchain
          riscv-toolchain.buildPackages.gcc
          riscv-toolchain.buildPackages.binutils
          kani
          pkgs.e2fsprogs
        ];

        commonEnv = {
          # Needed for bindgen
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        };

        hook = ''
          rm -rf musl headers/linux_headers headers/musl_headers

          ln -sf ${musl-riscv}/src musl
          ln -sf ${musl-riscv.linuxHeaders}/ headers/linux_headers
          ln -sf ${musl-riscv.dev}/include headers/musl_headers

          mkdir -p kernel/compiled_userspace_nix
          ln -sf "${dash}/bin/dash" "./kernel/compiled_userspace_nix/dash"
          ln -sf "${dash}/bin/dash" "./kernel/compiled_userspace_nix/sh"
          ln -sf "${doom-riscv}/bin/doom" "./kernel/compiled_userspace_nix/doom"

          just mcp-server
        '';

      in
      {
        devShells.default = pkgs.mkShell (
          commonEnv
          // {
            nativeBuildInputs = ciPackages ++ [
              pkgs.gdb
              pkgs.tmux
              pwndbg.packages.${system}.default
              pkgs.typos-lsp
              pkgs.dtc
              (pkgs.python3.withPackages (ps: [
                ps.pygdbmi
                ps.mcp
              ]))
            ];
            shellHook = hook;
          }
        );

        devShells.ci = pkgs.mkShell (
          commonEnv
          // {
            nativeBuildInputs = ciPackages;
            shellHook = hook;
          }
        );
      }
    );
}
