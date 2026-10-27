package `in`.arena360.hub

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Log

/**
 * The folder the owner chose for backups, for the Rust Hub (`backup_jni.rs` calls [probe] through
 * JNI). The system folder picker returns a document tree URI, not a path: [MainActivity] keeps
 * the read and write grant across restarts, and the Hub stores the URI as its backup location.
 * Every call blocks and runs on a Hub thread, never the UI one.
 */
object BackupFolderBridge {
  private const val TAG = "ArenaBackupFolder"
  private const val PROBE_PREFIX = ".arena360-write-test-"
  private const val FLAGS = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION

  /** The reasons [probe] answers with: the same words the Hub's backup location errors use. */
  const val NOT_FOUND = "not_found"
  const val NOT_DIR = "not_dir"
  const val NOT_WRITABLE = "not_writable"

  @Volatile private var context: Context? = null

  /** Called by the service before the Hub starts; idempotent. */
  @JvmStatic
  fun attach(app: Context) {
    context = app.applicationContext
  }

  /** Keeps the grant for a folder the owner just picked, across restarts and reboots. */
  @JvmStatic
  fun keep(app: Context, tree: Uri): Boolean =
    try {
      app.contentResolver.takePersistableUriPermission(tree, FLAGS)
      true
    } catch (e: SecurityException) {
      Log.w(TAG, "the folder grant could not be kept", e)
      false
    }

  /** Gives up the grants for every folder but [tree], so a replaced location does not linger. */
  @JvmStatic
  fun keepOnly(app: Context, tree: Uri) {
    for (grant in app.contentResolver.persistedUriPermissions) {
      if (grant.uri != tree) {
        try {
          app.contentResolver.releasePersistableUriPermission(grant.uri, FLAGS)
        } catch (e: SecurityException) {
          Log.w(TAG, "a folder grant could not be released", e)
        }
      }
    }
  }

  /**
   * Whether a probe file can be created and deleted in the folder at [location]: `null` when it
   * can, else [NOT_FOUND] (the drive is gone or the folder was deleted), [NOT_DIR] or
   * [NOT_WRITABLE] (the grant was revoked, or the folder is read-only).
   */
  @JvmStatic
  fun probe(location: String): String? {
    val app = context ?: return NOT_FOUND
    val tree = Uri.parse(location)
    val granted = app.contentResolver.persistedUriPermissions.any { it.uri == tree && it.isWritePermission }
    if (!granted) return NOT_WRITABLE
    return try {
      val folder = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
      val columns = arrayOf(DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_FLAGS)
      val (mime, flags) =
        app.contentResolver.query(folder, columns, null, null, null)?.use {
          if (it.moveToFirst()) it.getString(0) to it.getInt(1) else null
        } ?: return NOT_FOUND
      if (mime != DocumentsContract.Document.MIME_TYPE_DIR) return NOT_DIR
      if (flags and DocumentsContract.Document.FLAG_DIR_SUPPORTS_CREATE == 0) return NOT_WRITABLE
      val probe =
        DocumentsContract.createDocument(app.contentResolver, folder, "application/octet-stream", PROBE_PREFIX + System.nanoTime())
          ?: return NOT_WRITABLE
      if (DocumentsContract.deleteDocument(app.contentResolver, probe)) null else NOT_WRITABLE
    } catch (e: SecurityException) {
      NOT_WRITABLE
    } catch (e: Exception) {
      // A provider that went away (an unplugged USB drive) throws from its binder.
      Log.w(TAG, "the backup folder could not be checked", e)
      NOT_FOUND
    }
  }
}
