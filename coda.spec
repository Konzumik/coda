Name:           coda
Version:        0.1.0
Release:        1%{?dist}
Summary:        Cover-driven audio visualizer for the Kitty terminal

# coda: MIT; vendored crates: MIT, MIT OR Apache-2.0, Zlib, Unicode-3.0, BSD-3-Clause OR Apache-2.0
License:        MIT AND (MIT OR Apache-2.0) AND Zlib AND Unicode-3.0 AND (BSD-3-Clause OR Apache-2.0)
URL:            https://github.com/Konzumik/coda
# Both tarballs are produced by .copr/Makefile (git archive + cargo vendor)
Source0:        %{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.gz

BuildRequires:  cargo
BuildRequires:  rust >= 1.85
BuildRequires:  gcc

# parec captures the monitor of the default output
Requires:       pulseaudio-utils
Recommends:     kitty

%description
coda is a terminal spectrum visualizer for Kitty that paints itself in the
colors of the album art of whatever is playing. On track change it grabs the
cover over MPRIS, extracts a 3-5 color palette and smoothly blends the bars
into the new colors. Four modes: bars, mirror, mountains and ring.

%prep
%autosetup -n %{name}-%{version} -a1
mkdir -p .cargo
cat > .cargo/config.toml << 'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
export RUSTFLAGS="%{build_rustflags}"
cargo build --release --locked --offline

%install
install -Dpm 0755 target/release/%{name} %{buildroot}%{_bindir}/%{name}

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}

%changelog
* Wed Nov 04 2026 Konzumik <me@levsadylenko.xyz> - 0.1.0-1
- First package
