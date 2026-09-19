#!/bin/sh
# Builds and runs the DRI3 smoke against libvantage.so via glvnd-style paths:
# libEGL dispatches to our vendor JSON (see glvnd/egl_vendor.d).
cc -O1 -o dri3_smoke dri3_smoke.c \
  -I../../include \
  -lX11 \
  -L../../target/debug -lvantage_egl -lvantage_gles -lvantage_raster -lvantage_shader -lvantage_hal \
  -Wl,-rpath,"$PWD/../../target/debug"
