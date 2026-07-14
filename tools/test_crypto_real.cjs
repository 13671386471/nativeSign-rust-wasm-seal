// Test script: verify real SM2/SM3/SM4 implementations in WASM
const path = require('path');
const wasm = require(path.join(__dirname, '..', 'pkg_node', 'dianju_wasm_seal.js'));
const crypto = require('crypto');

async function main() {
    console.log('=== WASM module loaded ===\n');

    let pass = 0, fail = 0;
    function assert(cond, msg) {
        if (cond) { pass++; console.log(`  [PASS] ${msg}`); }
        else      { fail++; console.log(`  [FAIL] ${msg}`); }
    }

    // --- SM3 Hash Test ---
    console.log('--- SM3 Hash ---');
    {
        const data = Buffer.from('hello sm3', 'utf8');
        const hash = wasm.sm3_hash(data);
        console.log(`  SM3("hello sm3") = ${Buffer.from(hash).toString('hex')}`);
        assert(hash.length === 32, 'SM3 output should be 32 bytes');

        // Verify against known SM3 test vector: SM3("abc") 
        const abcHash = wasm.sm3_hash(Buffer.from('abc', 'utf8'));
        const expected = '66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0';
        const actual = Buffer.from(abcHash).toString('hex');
        console.log(`  SM3("abc") = ${actual}`);
        console.log(`  Expected   = ${expected}`);
        assert(actual === expected, 'SM3("abc") should match GM/T 0004-2012 test vector');
    }

    // --- SM2 Sign/Verify Test ---
    console.log('\n--- SM2 Sign/Verify ---');
    {
        const msg = Buffer.from('hello world, SM2 test message', 'utf8');
        const sig = wasm.sm2_sign(msg, new Uint8Array(0));
        const sigHex = Buffer.from(sig).toString('hex');
        console.log(`  Signature (DER hex, ${sig.length} bytes): ${sigHex.substring(0, 40)}...`);

        // Check DER format: should start with 0x30 (SEQUENCE tag)
        assert(sig[0] === 0x30, 'SM2 signature should be DER-encoded (start with 0x30)');
        assert(sig.length >= 70 && sig.length <= 72, `SM2 DER signature length should be 70-72, got ${sig.length}`);

        // Verify the signature
        const ok = wasm.sm2_verify(msg, sig, new Uint8Array(0));
        console.log(`  Verify result: ${ok}`);
        assert(ok === true, 'SM2 verify should return true for valid signature');

        // Verify with wrong data (should fail)
        const wrongMsg = Buffer.from('tampered message', 'utf8');
        const okWrong = wasm.sm2_verify(wrongMsg, sig, new Uint8Array(0));
        console.log(`  Verify with wrong data: ${okWrong}`);
        assert(okWrong === false, 'SM2 verify should return false for tampered data');

        // Verify with corrupted signature (should return false or error)
        const corruptSig = new Uint8Array(sig);
        corruptSig[5] ^= 0x01; // flip one bit
        let corruptResult;
        try {
            corruptResult = wasm.sm2_verify(msg, corruptSig, new Uint8Array(0));
        } catch(e) {
            corruptResult = false;
        }
        console.log(`  Verify with corrupted sig: ${corruptResult}`);
        assert(corruptResult === false, 'SM2 verify should reject corrupted signature');
    }

    // --- SM2 Pubkey Test ---
    console.log('\n--- SM2 Public Key ---');
    {
        const pubkey = wasm.get_sm2_pubkey();
        console.log(`  Pubkey (${pubkey.length} bytes): 04${Buffer.from(pubkey).toString('hex').substring(2, 20)}...`);
        assert(pubkey.length === 65, 'SM2 public key should be 65 bytes (04 || x || y)');
        assert(pubkey[0] === 0x04, 'SM2 public key should start with 0x04 (uncompressed)');
    }

    // --- SM4 CBC Encryption/Decryption Test ---
    console.log('\n--- SM4 CBC Encrypt/Decrypt ---');
    {
        const key = new Uint8Array(16).fill(0x01);
        const iv = new Uint8Array(16).fill(0x02);
        const plaintext = Buffer.from('hello world, this is a SM4 CBC test message', 'utf8');
        console.log(`  Plaintext (${plaintext.length} bytes): ${plaintext.toString()}`);

        const ciphertext = wasm.sm4_encrypt(plaintext, key, iv);
        console.log(`  Ciphertext (${ciphertext.length} bytes): ${Buffer.from(ciphertext).toString('hex').substring(0, 40)}...`);
        assert(ciphertext.length % 16 === 0, `SM4 ciphertext should be 16-byte aligned, got ${ciphertext.length}`);
        assert(ciphertext.length !== plaintext.length, 'SM4 ciphertext should differ from plaintext (CBC adds padding)');

        const decrypted = wasm.sm4_decrypt(ciphertext, key, iv);
        const decStr = Buffer.from(decrypted).toString('utf8');
        console.log(`  Decrypted (${decrypted.length} bytes): ${decStr}`);
        assert(decStr === plaintext.toString(), 'SM4 decrypt should match original plaintext');
    }

    // --- SM4 Edge Cases ---
    console.log('\n--- SM4 Edge Cases ---');
    {
        // Empty data
        const key = new Uint8Array(16).fill(0xAA);
        const iv = new Uint8Array(16).fill(0xBB);
        const ct = wasm.sm4_encrypt(new Uint8Array(0), key, iv);
        console.log(`  Empty data → ciphertext: ${ct.length} bytes`);
        assert(ct.length === 16, 'SM4 empty data should produce 16-byte ciphertext (PKCS#7 full block)');

        const pt = wasm.sm4_decrypt(ct, key, iv);
        assert(pt.length === 0, 'SM4 decrypt of empty should produce 0 bytes');

        // Exactly one block
        const block = new Uint8Array(16).fill(0x55);
        const ct2 = wasm.sm4_encrypt(block, key, iv);
        console.log(`  16-byte data → ciphertext: ${ct2.length} bytes`);
        assert(ct2.length === 32, 'SM4 16-byte data should produce 32-byte ciphertext (PKCS#7 adds full block)');

        const pt2 = wasm.sm4_decrypt(ct2, key, iv);
        assert(Buffer.from(pt2).equals(block), 'SM4 decrypt of 16-byte data should match');
    }

    // --- SM4 Error Handling ---
    console.log('\n--- SM4 Error Handling ---');
    {
        try {
            wasm.sm4_encrypt(Buffer.from('data'), new Uint8Array(15), new Uint8Array(16));
            assert(false, 'SM4 with 15-byte key should error');
        } catch(e) {
            assert(true, 'SM4 with wrong key length should error');
            console.log(`  Error: ${e.message || e}`);
        }

        try {
            wasm.sm4_encrypt(Buffer.from('data'), new Uint8Array(16), new Uint8Array(15));
            assert(false, 'SM4 with 15-byte IV should error');
        } catch(e) {
            assert(true, 'SM4 with wrong IV length should error');
        }
    }

    // --- Summary ---
    console.log('\n=== Summary ===');
    console.log(`  PASSED: ${pass}`);
    console.log(`  FAILED: ${fail}`);
    if (fail > 0) {
        console.log('\n❌ Some tests FAILED!');
        process.exit(1);
    } else {
        console.log('\n✅ All tests PASSED!');
    }
}

main().catch(e => {
    console.error('Fatal error:', e);
    process.exit(1);
});
