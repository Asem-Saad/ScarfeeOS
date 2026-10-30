# Version is the base release number, like scarfeeos-release (dnf's $releasever
# comes from the package that provides system-release).

Name:           scarfeeos-repos
Version:        44
Release:        0.1
Summary:        ScarfeeOS package repository definitions
License:        Apache-2.0
URL:            https://github.com/Asem-Saad/ScarfeeOS
BuildArch:      noarch

Source0:        LICENSE
Source1:        scarfeeos.repo

# Packages are signed with Fedora's keys, which fedora-gpg-keys installs.
Requires:       fedora-gpg-keys >= %{version}
Requires:       system-release(%{version})

Provides:       scarfeeos-repos(%{version}) = %{release}
# fedora-repos enables a Cisco H.264 repository by default (docs/ip-and-licensing.md rule 4).
Conflicts:      fedora-repos

%description
Package repository definitions for ScarfeeOS: the release and updates
repositories, with package signature checking on. It defines no repository for
patent-licensed codecs.

%prep
cp -p %{SOURCE0} .

%build

%install
install -d -m 0755 %{buildroot}%{_sysconfdir}/yum.repos.d
install -p -m 0644 %{SOURCE1} %{buildroot}%{_sysconfdir}/yum.repos.d/scarfeeos.repo

%files
%license LICENSE
%config(noreplace) %{_sysconfdir}/yum.repos.d/scarfeeos.repo

%changelog
* Fri Oct 30 2026 Asem Y Saad <asem.y.saad@outlook.com> - 44-0.1
- First ScarfeeOS repository package, replacing fedora-repos
