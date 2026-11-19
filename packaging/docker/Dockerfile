# Linux build box for Sokki: Arch Linux ARM, the same base as the Omarchy VM.
FROM menci/archlinuxarm:base-devel
RUN pacman -Syu --disable-sandbox --noconfirm --needed \
      rust clang cmake ninja pkgconf git \
      pipewire libpulse alsa-lib \
      vulkan-headers vulkan-icd-loader shaderc spirv-headers \
      webkit2gtk-4.1 gtk3 libappindicator-gtk3 gtk-layer-shell librsvg openssl \
      wireplumber pipewire-pulse pipewire-alsa \
      && pacman -Scc --disable-sandbox --noconfirm
ENV CARGO_HOME=/cargo
WORKDIR /src
