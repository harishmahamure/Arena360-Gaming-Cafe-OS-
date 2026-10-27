package `in`.arena360.hub

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.UnrecoverableKeyException
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Wraps the Hub's small secrets (the device token and the branch key) with an AES-GCM key that
 * lives in the Android Keystore and never leaves it: the key is created with the Keystore
 * provider, is only ever used through a [Cipher], and nothing here reads its encoded bytes.
 *
 * The Rust core calls [wrap] and [unwrap] through JNI (see `keystore_jni.rs`); the wrapped
 * bytes are `nonce || ciphertext` and Rust treats them as opaque.
 */
object KeystoreBridge {
  private const val PROVIDER = "AndroidKeyStore"
  private const val ALIAS = "in.arena360.hub.secrets.v1"
  private const val TRANSFORMATION = "AES/GCM/NoPadding"
  private const val NONCE_BYTES = 12
  private const val TAG_BITS = 128

  /** `nonce || ciphertext` for [plain]. Creates the key on first use. Throws on any failure. */
  @JvmStatic
  @Synchronized
  fun wrap(plain: ByteArray): ByteArray {
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, key(create = true) ?: throw GeneralSecurityException("no key"))
    return cipher.iv + cipher.doFinal(plain)
  }

  /**
   * The secret behind [wrapped], or `null` when the key is lost: the alias is gone (the
   * Keystore was reset or the app data came from another device), the key was invalidated, or
   * the bytes do not authenticate under it. Any other failure throws, so a transient fault is
   * never mistaken for a lost key.
   */
  @JvmStatic
  fun unwrap(wrapped: ByteArray): ByteArray? {
    if (wrapped.size <= NONCE_BYTES + TAG_BITS / 8) return null
    val key = try {
      key(create = false)
    } catch (e: UnrecoverableKeyException) {
      null
    } ?: return null
    return try {
      val cipher = Cipher.getInstance(TRANSFORMATION)
      cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, wrapped, 0, NONCE_BYTES))
      cipher.doFinal(wrapped, NONCE_BYTES, wrapped.size - NONCE_BYTES)
    } catch (e: AEADBadTagException) {
      null
    } catch (e: KeyPermanentlyInvalidatedException) {
      null
    }
  }

  @Synchronized
  private fun key(create: Boolean): SecretKey? {
    val store = KeyStore.getInstance(PROVIDER).apply { load(null) }
    (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
    if (!create) return null
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
    generator.init(
      KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build(),
    )
    return generator.generateKey()
  }
}
