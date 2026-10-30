# Version is the base release number the package repositories are keyed on
# (dnf takes $releasever from the package that provides system-release), not
# the ScarfeeOS version; that one (0.0.1) is in the files listed in Source1 to Source4.

Name:           scarfeeos-release
Version:        44
Release:        0.1
Summary:        ScarfeeOS release files
License:        Apache-2.0
URL:            https://github.com/Asem-Saad/ScarfeeOS
BuildArch:      noarch

Source0:        LICENSE
Source1:        os-release
Source2:        scarfeeos-release
Source3:        issue
Source4:        system-release-cpe
Source5:        90-scarfeeos.preset
Source6:        90-scarfeeos-user.preset

# The installed system still takes its package repositories and signing keys
# from the Fedora packages; see os-image/README.md, "Branded packages".
Requires:       fedora-repos(%{version})
Recommends:     scarfeeos-logos

Provides:       system-release = %{version}-%{release}
Provides:       system-release(%{version}) = %{release}
Conflicts:      fedora-release
Conflicts:      fedora-release-common
Conflicts:      generic-release
Conflicts:      generic-release-common
Conflicts:      system-release

%description
Release files for ScarfeeOS: the os-release identity, the login banner, the
release and CPE names, and the default systemd service policy.

%prep
cp -p %{SOURCE0} .

%build

%install
install -d -m 0755 %{buildroot}%{_prefix}/lib/systemd/system-preset
install -d -m 0755 %{buildroot}%{_prefix}/lib/systemd/user-preset
install -d -m 0755 %{buildroot}%{_sysconfdir}
install -p -m 0644 %{SOURCE1} %{buildroot}%{_prefix}/lib/os-release
install -p -m 0644 %{SOURCE2} %{buildroot}%{_prefix}/lib/scarfeeos-release
install -p -m 0644 %{SOURCE3} %{buildroot}%{_prefix}/lib/issue
ln -s issue %{buildroot}%{_prefix}/lib/issue.net
install -p -m 0644 %{SOURCE4} %{buildroot}%{_prefix}/lib/system-release-cpe
install -p -m 0644 %{SOURCE5} %{buildroot}%{_prefix}/lib/systemd/system-preset/90-scarfeeos.preset
install -p -m 0644 %{SOURCE6} %{buildroot}%{_prefix}/lib/systemd/user-preset/90-scarfeeos-user.preset
ln -s ../usr/lib/os-release %{buildroot}%{_sysconfdir}/os-release
ln -s ../usr/lib/scarfeeos-release %{buildroot}%{_sysconfdir}/scarfeeos-release
ln -s scarfeeos-release %{buildroot}%{_sysconfdir}/system-release
ln -s ../usr/lib/issue %{buildroot}%{_sysconfdir}/issue
ln -s ../usr/lib/issue.net %{buildroot}%{_sysconfdir}/issue.net
ln -s ../usr/lib/system-release-cpe %{buildroot}%{_sysconfdir}/system-release-cpe

%files
%license LICENSE
%{_prefix}/lib/os-release
%{_prefix}/lib/scarfeeos-release
%{_prefix}/lib/issue
%{_prefix}/lib/issue.net
%{_prefix}/lib/system-release-cpe
%{_prefix}/lib/systemd/system-preset/90-scarfeeos.preset
%{_prefix}/lib/systemd/user-preset/90-scarfeeos-user.preset
%{_sysconfdir}/os-release
%{_sysconfdir}/scarfeeos-release
%{_sysconfdir}/system-release
%{_sysconfdir}/issue
%{_sysconfdir}/issue.net
%{_sysconfdir}/system-release-cpe

%changelog
* Fri Oct 30 2026 Asem Y Saad <asem.y.saad@outlook.com> - 44-0.1
- First ScarfeeOS release package, replacing generic-release
