Name:           scarfeeos-logos
Version:        0.0.1
Release:        1
Summary:        ScarfeeOS logo
License:        Apache-2.0
URL:            https://github.com/Asem-Saad/ScarfeeOS
BuildArch:      noarch

Source0:        LICENSE
Source1:        scarfeeos-logo.svg

Requires:       hicolor-icon-theme

%description
The ScarfeeOS logo: an original two-color scarf mark drawn for this project.
It is installed as the icon named scarfeeos-logo-icon, which os-release
refers to in its LOGO field.

%prep
cp -p %{SOURCE0} .

%build

%install
install -d -m 0755 %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -d -m 0755 %{buildroot}%{_datadir}/scarfeeos-logos
install -p -m 0644 %{SOURCE1} %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/scarfeeos-logo-icon.svg
ln -s ../icons/hicolor/scalable/apps/scarfeeos-logo-icon.svg %{buildroot}%{_datadir}/scarfeeos-logos/scarfeeos-logo.svg

%files
%license LICENSE
%{_datadir}/icons/hicolor/scalable/apps/scarfeeos-logo-icon.svg
%{_datadir}/scarfeeos-logos/

%changelog
* Fri Oct 30 2026 Asem Y Saad <asem.y.saad@outlook.com> - 0.0.1-1
- First ScarfeeOS logo package
