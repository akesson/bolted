# repro-05 — draft 05 (negative result)

Draft 05 claimed `Result<ClassHandle, E>` cannot be returned from a throwing `#[export]` method
(`Handle: WireEncode` not satisfied) at boltffi 0.27.3. **This repro shows it compiles** — at both
0.27.5 and 0.27.3.

```sh
cd upstream/boltffi/repro-05

# as-is (boltffi 0.27.5)
cargo build            # → Finished (no WireEncode error)

# flip the pin to 0.27.3 and rebuild
sed -i '' 's/boltffi = "0.27.5"/boltffi = "=0.27.3"/' Cargo.toml
rm -f Cargo.lock
cargo build            # → Finished (still no WireEncode error)
```

Both succeed. In step 15 M4 the same negative result held for the **real** `spike-profile-ffi`
signature `ProfileStoreFfi::try_restore(&self, ..) -> Result<ProfileDraftFfi, SubmitErrorFfi>`, with
and without `--cfg boltffi_binding_expansion`, at both versions (Cargo.lock version verified each
time). Conclusion: the reported failure is not reproducible → do not file. See
`../05-throwing-method-cannot-return-class-handle.md`.
