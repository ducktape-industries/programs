# Features rustc emits for wasm32-unknown-unknown. Both export stripping and
# optimization read this list, so the two Binaryen passes accept the same code.
wasm_features='--enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int --enable-multivalue --enable-reference-types'
