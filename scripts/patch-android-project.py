#!/usr/bin/env python3
"""Patch Tauri's generated Android project (CI / local builds).

1. Configures MainActivity edge-to-edge system bar + cutout + IME keyboard
   insets and light ivory bars (#FCFCF6 / #F6F6EF).
2. Adds res/xml/dadaduck_file_paths.xml + FileProvider + legacy API<=28 storage
   permission declaration to AndroidManifest.xml.
3. Generates AndroidStoragePlugin.kt (@TauriPlugin) for MediaStore.Downloads
   publishing, SAF directory picking (ACTION_OPEN_DOCUMENT_TREE), content://
   verification, ACTION_VIEW opening, ACTION_SEND sharing, file deletion, and
   clipboard reading.
"""

from __future__ import annotations

import pathlib
import re
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ANDROID_DIR = ROOT / "src-tauri" / "gen" / "android"
APP_MAIN_DIR = ANDROID_DIR / "app" / "src" / "main"
RES_DIR = APP_MAIN_DIR / "res"
ICONS_ANDROID_DIR = ROOT / "src-tauri" / "icons" / "android"
MANIFEST_PATH = APP_MAIN_DIR / "AndroidManifest.xml"
APP_GRADLE_PATH = ANDROID_DIR / "app" / "build.gradle.kts"
ANDROID_SDK_API = 36
FILE_PATHS_XML = APP_MAIN_DIR / "res" / "xml" / "dadaduck_file_paths.xml"

FILE_PATHS_CONTENT = """<?xml version="1.0" encoding="utf-8"?>
<paths xmlns:android="http://schemas.android.com/apk/res/android">
    <files-path name="app_files" path="." />
    <cache-path name="app_cache" path="." />
    <external-path name="external_storage" path="." />
    <external-files-path name="external_app_files" path="." />
    <external-cache-path name="external_app_cache" path="." />
</paths>
"""

PROVIDER_BLOCK = """        <provider
            android:name="androidx.core.content.FileProvider"
            android:authorities="${applicationId}.dadaduck.fileprovider"
            android:exported="false"
            android:grantUriPermissions="true">
            <meta-data
                android:name="android.support.FILE_PROVIDER_PATHS"
                android:resource="@xml/dadaduck_file_paths" />
        </provider>
"""

LEGACY_WRITE_PERM = (
    '    <uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE"'
    ' android:maxSdkVersion="28" />'
)


def render_main_activity(package_name: str) -> str:
    return f"""package {package_name}

import android.graphics.Color
import android.os.Bundle
import android.view.View
import android.view.WindowManager
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {{
  override fun onCreate(savedInstanceState: Bundle?) {{
    super.onCreate(savedInstanceState)

    val ivory = Color.parseColor("#FCFCF6")
    val navBg = Color.parseColor("#F6F6EF")
    window.decorView.setBackgroundColor(ivory)
    @Suppress("DEPRECATION")
    run {{
      window.statusBarColor = ivory
      window.navigationBarColor = navBg
      window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
    }}

    WindowCompat.setDecorFitsSystemWindows(window, false)
    val insetsController = WindowCompat.getInsetsController(window, window.decorView)
    insetsController.isAppearanceLightStatusBars = true
    insetsController.isAppearanceLightNavigationBars = true

    val content = findViewById<View>(android.R.id.content)
    content.setBackgroundColor(ivory)
    ViewCompat.setOnApplyWindowInsetsListener(content) {{ view, windowInsets ->
      val bars = windowInsets.getInsets(
        WindowInsetsCompat.Type.systemBars()
          or WindowInsetsCompat.Type.displayCutout()
          or WindowInsetsCompat.Type.ime()
      )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }}
    ViewCompat.requestApplyInsets(content)
  }}
}}
"""


def render_storage_plugin(package_name: str) -> str:
    return f"""package {package_name}

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentResolver
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.util.concurrent.Executors

@InvokeArg
class PublishFileArgs {{
  lateinit var tempPath: String
  var subdirSegments: Array<String> = emptyArray()
  lateinit var fileName: String
  var mimeType: String? = null
  var storageMode: String? = null
  var treeUri: String? = null
}}

@InvokeArg
class LocationArgs {{
  lateinit var location: String
  var mimeType: String? = null
  var title: String? = null
}}

@InvokeArg
class SensitiveClipboardArgs {{
  lateinit var text: String
}}

@TauriPlugin
class AndroidStoragePlugin(private val activity: Activity) : Plugin(activity) {{
  private val ioExecutor = Executors.newSingleThreadExecutor()

  @Command
  fun publishFile(invoke: Invoke) {{
    val args = try {{
      invoke.parseArgs(PublishFileArgs::class.java)
    }} catch (e: Throwable) {{
      invoke.reject("参数解析失败: ${{e.message}}")
      return
    }}

    ioExecutor.execute {{
      try {{
        val srcFile = File(args.tempPath)
        if (!srcFile.exists() || !srcFile.isFile) {{
          invoke.reject("临时文件不存在，请重新下载")
          return@execute
        }}
        val expectedBytes = srcFile.length()
        if (expectedBytes <= 0L) {{
          invoke.reject("临时文件为空，已取消保存")
          return@execute
        }}

        val cleanSegments = args.subdirSegments
          .map {{ sanitizeSegment(it) }}
          .filter {{ it.isNotEmpty() }}
        val desiredName = sanitizeFileName(args.fileName)
        val mime = resolveMime(desiredName, args.mimeType)
        val mode = args.storageMode ?: "media_store_downloads"

        val result = if (mode == "saf_tree" && !args.treeUri.isNullOrBlank()) {{
          publishToSafTree(srcFile, expectedBytes, args.treeUri!!, cleanSegments, desiredName, mime)
        }} else {{
          publishToDownloads(srcFile, expectedBytes, cleanSegments, desiredName, mime)
        }}
        invoke.resolve(result)
      }} catch (e: Throwable) {{
        invoke.reject(e.message?.takeIf {{ it.isNotBlank() }} ?: e.javaClass.simpleName)
      }}
    }}
  }}

  @Command
  fun pickDirectory(invoke: Invoke) {{
    try {{
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {{
        addFlags(
          Intent.FLAG_GRANT_READ_URI_PERMISSION
            or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            or Intent.FLAG_GRANT_PREFIX_URI_PERMISSION
        )
      }}
      startActivityForResult(invoke, intent, "pickDirectoryResult")
    }} catch (e: Throwable) {{
      invoke.reject(e.message ?: "无法打开系统目录选择器")
    }}
  }}

  @ActivityCallback
  fun pickDirectoryResult(invoke: Invoke, result: ActivityResult) {{
    val ret = JSObject()
    if (result.resultCode != Activity.RESULT_OK) {{
      ret.put("cancelled", true)
      invoke.resolve(ret)
      return
    }}
    val uri = result.data?.data
    if (uri == null) {{
      ret.put("cancelled", true)
      invoke.resolve(ret)
      return
    }}
    try {{
      val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
      activity.contentResolver.takePersistableUriPermission(uri, flags)
      val label = formatTreeDisplayName(uri)
      ret.put("cancelled", false)
      ret.put("treeUri", uri.toString())
      ret.put("displayName", label)
      invoke.resolve(ret)
    }} catch (e: Throwable) {{
      invoke.reject("无法持久化文件夹权限: ${{e.message ?: e.javaClass.simpleName}}")
    }}
  }}

  @Command
  fun checkDocumentAccess(invoke: Invoke) {{
    val args = try {{
      invoke.parseArgs(LocationArgs::class.java)
    }} catch (e: Throwable) {{
      invoke.reject("参数解析失败: ${{e.message}}")
      return
    }}
    ioExecutor.execute {{
      val ret = JSObject()
      val loc = args.location.trim()
      if (loc.isEmpty()) {{
        ret.put("exists", false)
        ret.put("sizeBytes", 0)
        invoke.resolve(ret)
        return@execute
      }}
      if (loc.startsWith("content://")) {{
        val uri = Uri.parse(loc)
        var exists = false
        var size = 0L
        try {{
          activity.contentResolver.openAssetFileDescriptor(uri, "r")?.use {{ afd ->
            exists = true
            if (afd.length >= 0L) {{
              size = afd.length
            }}
          }}
        }} catch (_: Throwable) {{
          exists = false
        }}
        if (exists && size <= 0L) {{
          size = queryOpenableSize(uri)
        }}
        ret.put("exists", exists)
        ret.put("sizeBytes", size)
        invoke.resolve(ret)
      }} else {{
        val file = File(loc)
        val exists = file.exists() && file.isFile
        ret.put("exists", exists)
        ret.put("sizeBytes", if (exists) file.length() else 0L)
        invoke.resolve(ret)
      }}
    }}
  }}

  @Command
  fun openDocument(invoke: Invoke) {{
    val args = try {{
      invoke.parseArgs(LocationArgs::class.java)
    }} catch (e: Throwable) {{
      invoke.reject("参数解析失败: ${{e.message}}")
      return
    }}
    try {{
      val resolved = resolveShareableUri(args.location)
      val uri = resolved.first
      val nameHint = resolved.second
      if (uri == null) {{
        invoke.reject("文件已被移动或删除", "MISSING")
        return
      }}
      val mime = resolveMime(nameHint, args.mimeType)
      val intent = Intent(Intent.ACTION_VIEW).apply {{
        setDataAndType(uri, mime)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      }}
      activity.startActivity(intent)
      invoke.resolve()
    }} catch (_: ActivityNotFoundException) {{
      invoke.reject("手机暂未安装可打开此类文件的阅读器应用", "NO_VIEWER")
    }} catch (e: Throwable) {{
      invoke.reject(e.message ?: e.javaClass.simpleName)
    }}
  }}

  @Command
  fun shareDocument(invoke: Invoke) {{
    val args = try {{
      invoke.parseArgs(LocationArgs::class.java)
    }} catch (e: Throwable) {{
      invoke.reject("参数解析失败: ${{e.message}}")
      return
    }}
    try {{
      val resolved = resolveShareableUri(args.location)
      val uri = resolved.first
      val nameHint = resolved.second
      if (uri == null) {{
        invoke.reject("文件已被移动或删除", "MISSING")
        return
      }}
      val mime = resolveMime(nameHint, args.mimeType)
      val sendIntent = Intent(Intent.ACTION_SEND).apply {{
        type = mime
        putExtra(Intent.EXTRA_STREAM, uri)
        clipData = ClipData.newRawUri(nameHint.ifEmpty {{ "送达文书" }}, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }}
      val chooser = Intent.createChooser(sendIntent, args.title ?: "分享送达文书").apply {{
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      }}
      activity.startActivity(chooser)
      invoke.resolve()
    }} catch (e: Throwable) {{
      invoke.reject(e.message ?: e.javaClass.simpleName)
    }}
  }}

  @Command
  fun deleteDocument(invoke: Invoke) {{
    val args = try {{
      invoke.parseArgs(LocationArgs::class.java)
    }} catch (e: Throwable) {{
      invoke.reject("参数解析失败: ${{e.message}}")
      return
    }}
    ioExecutor.execute {{
      val loc = args.location.trim()
      if (loc.isEmpty()) {{
        invoke.reject("文件路径为空")
        return@execute
      }}
      try {{
        val deleted = if (loc.startsWith("content://")) {{
          val uri = Uri.parse(loc)
          val alreadyMissing = try {{
            activity.contentResolver.openAssetFileDescriptor(uri, "r")?.use {{ false }} ?: true
          }} catch (_: Throwable) {{
            true
          }}
          if (alreadyMissing) {{
            true
          }} else if (DocumentsContract.isDocumentUri(activity, uri)) {{
            var ok = try {{
              DocumentsContract.deleteDocument(activity.contentResolver, uri)
            }} catch (_: Throwable) {{
              false
            }}
            if (!ok) {{
              ok = try {{
                activity.contentResolver.delete(uri, null, null) > 0
              }} catch (_: Throwable) {{
                false
              }}
            }}
            ok
          }} else {{
            activity.contentResolver.delete(uri, null, null) > 0
          }}
        }} else {{
          val file = File(loc)
          !file.exists() || file.delete() || !file.exists()
        }}
        if (!deleted) {{
          invoke.reject("无法删除该文件，请检查存储权限或在系统文件管理中手动删除")
          return@execute
        }}
        val ret = JSObject()
        ret.put("deleted", true)
        invoke.resolve(ret)
      }} catch (_: Throwable) {{
        invoke.reject("无法删除该文件，请检查存储权限或在系统文件管理中手动删除")
      }}
    }}
  }}

  @Command
  fun copySensitiveText(invoke: Invoke) {{
    val args = invoke.parseArgs(SensitiveClipboardArgs::class.java)
    Handler(Looper.getMainLooper()).post {{
      try {{
        val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = ClipData.newPlainText("提取码", args.text)
        clip.description.extras = PersistableBundle().apply {{
          putBoolean("android.content.extra.IS_SENSITIVE", true)
        }}
        clipboard.setPrimaryClip(clip)
        invoke.resolve(JSObject())
      }} catch (_: Throwable) {{
        invoke.reject("无法复制提取码")
      }}
    }}
  }}

  @Command
  fun readClipboardText(invoke: Invoke) {{
    Handler(Looper.getMainLooper()).post {{
      try {{
        val cm = activity.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        val clip = cm?.primaryClip
        val text = if (clip != null && clip.itemCount > 0) {{
          clip.getItemAt(0).coerceToText(activity)?.toString() ?: ""
        }} else {{
          ""
        }}
        val ret = JSObject()
        ret.put("text", text)
        invoke.resolve(ret)
      }} catch (e: Throwable) {{
        invoke.reject(e.message ?: "无法读取剪贴板")
      }}
    }}
  }}

  private fun publishToDownloads(
    srcFile: File,
    expectedBytes: Long,
    subdirSegments: List<String>,
    desiredName: String,
    mime: String
  ): JSObject {{
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {{
      val resolver = activity.contentResolver
      val parts = mutableListOf(Environment.DIRECTORY_DOWNLOADS, "DadaDuck送达文书")
      parts.addAll(subdirSegments)
      val relPathNoSlash = parts.joinToString("/")
      val relPathWithSlash = "$relPathNoSlash/"

      val existingNames = queryMediaStoreNames(resolver, relPathWithSlash, relPathNoSlash)
      val chosenName = pickUniqueFileName(desiredName, existingNames)

      val values = ContentValues().apply {{
        put(MediaStore.MediaColumns.DISPLAY_NAME, chosenName)
        put(MediaStore.MediaColumns.MIME_TYPE, mime)
        put(MediaStore.MediaColumns.RELATIVE_PATH, relPathWithSlash)
        put(MediaStore.MediaColumns.IS_PENDING, 1)
      }}

      val collection = MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
      val itemUri = resolver.insert(collection, values)
        ?: throw IllegalStateException("系统无法在「下载」目录创建文书条目")

      try {{
        var copied = 0L
        resolver.openOutputStream(itemUri, "w")?.use {{ out ->
          FileInputStream(srcFile).use {{ inp ->
            val buf = ByteArray(65536)
            while (true) {{
              val n = inp.read(buf)
              if (n <= 0) break
              out.write(buf, 0, n)
              copied += n
            }}
            out.flush()
          }}
        }} ?: throw IllegalStateException("无法写入系统「下载」目录")

        if (copied != expectedBytes) {{
          throw IllegalStateException("写入下载目录字节数不一致 ($copied / $expectedBytes)")
        }}

        val publishValues = ContentValues().apply {{
          put(MediaStore.MediaColumns.IS_PENDING, 0)
        }}
        val updatedRows = resolver.update(itemUri, publishValues, null, null)
        if (updatedRows <= 0) {{
          throw IllegalStateException("下载目录未能完成发布状态更新")
        }}

        val verified = try {{
          resolver.openAssetFileDescriptor(itemUri, "r")?.use {{ afd ->
            afd.length < 0L || afd.length == copied
          }} ?: false
        }} catch (_: Throwable) {{
          false
        }}
        if (!verified) {{
          throw IllegalStateException("下载目录写入后校验失败，文件不可读")
        }}

        val actualName = queryOpenableName(itemUri) ?: chosenName
        val friendlySub = (listOf("下载", "DadaDuck送达文书") + subdirSegments + actualName).joinToString("/")
        return JSObject().apply {{
          put("contentUri", itemUri.toString())
          put("displayPath", friendlySub)
          put("finalFileName", actualName)
          put("sizeBytes", copied)
        }}
      }} catch (e: Throwable) {{
        try {{
          resolver.delete(itemUri, null, null)
        }} catch (_: Throwable) {{
        }}
        throw e
      }}
    }} else {{
      @Suppress("DEPRECATION")
      val base = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
      var dir = File(base, "DadaDuck送达文书")
      for (seg in subdirSegments) {{
        dir = File(dir, seg)
      }}
      if (!dir.exists() && !dir.mkdirs()) {{
        throw IllegalStateException("无法创建下载目录: ${{dir.absolutePath}}")
      }}
      val existingNames = dir.listFiles()?.map {{ it.name }}?.toSet() ?: emptySet()
      val chosenName = pickUniqueFileName(desiredName, existingNames)
      val destFile = File(dir, chosenName)
      var copied = 0L
      try {{
        FileOutputStream(destFile).use {{ out ->
          FileInputStream(srcFile).use {{ inp ->
            val buf = ByteArray(65536)
            while (true) {{
              val n = inp.read(buf)
              if (n <= 0) break
              out.write(buf, 0, n)
              copied += n
            }}
            out.flush()
          }}
        }}
        if (copied != expectedBytes) {{
          destFile.delete()
          throw IllegalStateException("写入文件字节数不一致")
        }}
        val authority = "${{activity.applicationContext.packageName}}.dadaduck.fileprovider"
        val contentUri = FileProvider.getUriForFile(activity, authority, destFile)
        val friendlySub = (listOf("下载", "DadaDuck送达文书") + subdirSegments + chosenName).joinToString("/")
        return JSObject().apply {{
          put("contentUri", contentUri.toString())
          put("displayPath", friendlySub)
          put("finalFileName", chosenName)
          put("sizeBytes", copied)
        }}
      }} catch (e: Throwable) {{
        destFile.delete()
        throw e
      }}
    }}
  }}

  private fun publishToSafTree(
    srcFile: File,
    expectedBytes: Long,
    treeUriStr: String,
    subdirSegments: List<String>,
    desiredName: String,
    mime: String
  ): JSObject {{
    val resolver = activity.contentResolver
    val treeUri = Uri.parse(treeUriStr)
    val rootDocId = DocumentsContract.getTreeDocumentId(treeUri)
    var currentDocUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, rootDocId)
    var currentDocId = rootDocId

    for (seg in subdirSegments) {{
      val existingChildId = findChildDocument(resolver, treeUri, currentDocId, seg, expectDir = true)
      if (existingChildId != null) {{
        currentDocId = existingChildId
        currentDocUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, existingChildId)
      }} else {{
        val created = DocumentsContract.createDocument(
          resolver,
          currentDocUri,
          DocumentsContract.Document.MIME_TYPE_DIR,
          seg
        ) ?: throw IllegalStateException("无法在所选目录创建子文件夹「$seg」，请检查文件夹授权")
        currentDocUri = created
        currentDocId = DocumentsContract.getDocumentId(created)
      }}
    }}

    val existingNames = listChildNames(resolver, treeUri, currentDocId)
    val chosenName = pickUniqueFileName(desiredName, existingNames)
    val docUri = DocumentsContract.createDocument(resolver, currentDocUri, mime, chosenName)
      ?: throw IllegalStateException("无法在所选目录创建文件，请检查授权是否仍然有效")

    try {{
      var copied = 0L
      resolver.openOutputStream(docUri, "w")?.use {{ out ->
        FileInputStream(srcFile).use {{ inp ->
          val buf = ByteArray(65536)
          while (true) {{
            val n = inp.read(buf)
            if (n <= 0) break
            out.write(buf, 0, n)
            copied += n
          }}
          out.flush()
        }}
      }} ?: throw IllegalStateException("无法打开目标文件输出流")

      if (copied != expectedBytes) {{
        throw IllegalStateException("写入目标目录字节数不一致 ($copied / $expectedBytes)")
      }}

      val verified = try {{
        resolver.openAssetFileDescriptor(docUri, "r")?.use {{ afd ->
          afd.length < 0L || afd.length == copied
        }} ?: false
      }} catch (_: Throwable) {{
        false
      }}
      if (!verified) {{
        throw IllegalStateException("写入目标目录后校验失败，文件不可读")
      }}

      val actualName = queryOpenableName(docUri) ?: chosenName
      val treeLabel = formatTreeDisplayName(treeUri)
      val displayPath = (listOf(treeLabel) + subdirSegments + actualName).joinToString("/")
      return JSObject().apply {{
        put("contentUri", docUri.toString())
        put("displayPath", displayPath)
        put("finalFileName", actualName)
        put("sizeBytes", copied)
      }}
    }} catch (e: Throwable) {{
      try {{
        DocumentsContract.deleteDocument(resolver, docUri)
      }} catch (_: Throwable) {{
      }}
      throw e
    }}
  }}

  private fun queryMediaStoreNames(
    resolver: ContentResolver,
    relWithSlash: String,
    relNoSlash: String
  ): Set<String> {{
    val names = HashSet<String>()
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return names
    val collection = MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
    val projection = arrayOf(MediaStore.MediaColumns.DISPLAY_NAME)
    val selection = "${{MediaStore.MediaColumns.RELATIVE_PATH}} IN (?, ?)"
    resolver.query(collection, projection, selection, arrayOf(relWithSlash, relNoSlash), null)?.use {{ cursor ->
      val idx = cursor.getColumnIndex(MediaStore.MediaColumns.DISPLAY_NAME)
      if (idx >= 0) {{
        while (cursor.moveToNext()) {{
          cursor.getString(idx)?.let {{ names.add(it) }}
        }}
      }}
    }}
    return names
  }}

  private fun listChildNames(resolver: ContentResolver, treeUri: Uri, parentDocId: String): Set<String> {{
    val names = HashSet<String>()
    val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
    val projection = arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
    resolver.query(childrenUri, projection, null, null, null)?.use {{ cursor ->
      val nameIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
      if (nameIdx >= 0) {{
        while (cursor.moveToNext()) {{
          cursor.getString(nameIdx)?.let {{ names.add(it) }}
        }}
      }}
    }}
    return names
  }}

  private fun findChildDocument(
    resolver: ContentResolver,
    treeUri: Uri,
    parentDocId: String,
    targetName: String,
    expectDir: Boolean
  ): String? {{
    val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
    val projection = arrayOf(
      DocumentsContract.Document.COLUMN_DOCUMENT_ID,
      DocumentsContract.Document.COLUMN_DISPLAY_NAME,
      DocumentsContract.Document.COLUMN_MIME_TYPE
    )
    resolver.query(childrenUri, projection, null, null, null)?.use {{ cursor ->
      val idIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
      val nameIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
      val mimeIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_MIME_TYPE)
      while (cursor.moveToNext()) {{
        val name = if (nameIdx >= 0) cursor.getString(nameIdx) else null
        if (name == targetName) {{
          val mime = if (mimeIdx >= 0) cursor.getString(mimeIdx) else null
          if (!expectDir || mime == DocumentsContract.Document.MIME_TYPE_DIR) {{
            return if (idIdx >= 0) cursor.getString(idIdx) else null
          }}
        }}
      }}
    }}
    return null
  }}

  private fun resolveShareableUri(rawLocation: String): Pair<Uri?, String> {{
    val loc = rawLocation.trim()
    if (loc.isEmpty()) return Pair(null, "")
    if (loc.startsWith("content://")) {{
      val uri = Uri.parse(loc)
      val exists = try {{
        activity.contentResolver.openAssetFileDescriptor(uri, "r")?.use {{ true }} ?: false
      }} catch (_: Throwable) {{
        false
      }}
      if (!exists) return Pair(null, "")
      val name = queryOpenableName(uri) ?: ""
      return Pair(uri, name)
    }}
    val file = File(loc)
    if (!file.exists() || !file.isFile) return Pair(null, "")
    val authority = "${{activity.applicationContext.packageName}}.dadaduck.fileprovider"
    return Pair(FileProvider.getUriForFile(activity, authority, file), file.name)
  }}

  private fun queryOpenableName(uri: Uri): String? {{
    return try {{
      activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {{ c ->
        if (c.moveToFirst()) c.getString(0) else null
      }}
    }} catch (_: Throwable) {{
      null
    }}
  }}

  private fun queryOpenableSize(uri: Uri): Long {{
    return try {{
      activity.contentResolver.query(uri, arrayOf(OpenableColumns.SIZE), null, null, null)?.use {{ c ->
        if (c.moveToFirst() && !c.isNull(0)) c.getLong(0) else 0L
      }} ?: 0L
    }} catch (_: Throwable) {{
      0L
    }}
  }}

  private fun formatTreeDisplayName(treeUri: Uri): String {{
    return try {{
      val docId = DocumentsContract.getTreeDocumentId(treeUri)
      when {{
        docId.startsWith("primary:") -> {{
          val rel = docId.removePrefix("primary:").trim('/')
          if (rel.isEmpty()) "手机内部存储" else "内部存储/$rel"
        }}
        docId.contains(':') -> docId.replace(':', '/')
        else -> docId
      }}
    }} catch (_: Throwable) {{
      treeUri.lastPathSegment ?: "已授权文件夹"
    }}
  }}

  private fun pickUniqueFileName(desiredName: String, existing: Set<String>): String {{
    val lowerSet = existing.map {{ it.lowercase() }}.toSet()
    if (!lowerSet.contains(desiredName.lowercase())) return desiredName
    val dotIdx = desiredName.lastIndexOf('.')
    val stem = if (dotIdx > 0) desiredName.substring(0, dotIdx) else desiredName
    val ext = if (dotIdx > 0) desiredName.substring(dotIdx) else ""
    var n = 1
    while (true) {{
      val candidate = "$stem ($n)$ext"
      if (!lowerSet.contains(candidate.lowercase())) return candidate
      n += 1
    }}
  }}

  private fun sanitizeSegment(seg: String): String {{
    return seg
      .replace(Regex("[\\\\/:*?\\"<>|]"), "_")
      .trim()
      .trim('.')
  }}

  private fun sanitizeFileName(name: String): String {{
    val cleaned = sanitizeSegment(name)
    return if (cleaned.isEmpty()) "送达文书.pdf" else cleaned
  }}

  private fun resolveMime(fileName: String, hint: String?): String {{
    if (!hint.isNullOrBlank() && hint.contains('/') && hint != "application/octet-stream") return hint
    val ext = fileName.substringAfterLast('.', "").lowercase()
    return when (ext) {{
      "pdf" -> "application/pdf"
      "ofd" -> "application/ofd"
      "doc" -> "application/msword"
      "docx" -> "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
      "xls" -> "application/vnd.ms-excel"
      "xlsx" -> "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
      "ppt" -> "application/vnd.ms-powerpoint"
      "pptx" -> "application/vnd.openxmlformats-officedocument.presentationml.presentation"
      "wps" -> "application/vnd.ms-works"
      "rtf" -> "application/rtf"
      "txt" -> "text/plain"
      "zip" -> "application/zip"
      "rar" -> "application/vnd.rar"
      "7z" -> "application/x-7z-compressed"
      "jpg", "jpeg" -> "image/jpeg"
      "png" -> "image/png"
      "gif" -> "image/gif"
      "bmp" -> "image/bmp"
      "webp" -> "image/webp"
      "tif", "tiff" -> "image/tiff"
      "" -> "application/pdf"
      else -> MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
    }}
  }}
}}
"""


def main() -> int:
    if not ANDROID_DIR.exists():
        print(f"Android generated directory not found: {ANDROID_DIR}", file=sys.stderr)
        return 1

    gradle_text = APP_GRADLE_PATH.read_text(encoding="utf-8")
    patched_gradle = gradle_text
    for setting in ("compileSdk", "targetSdk"):
        patched_gradle, count = re.subn(
            rf"(?m)^([ \t]*{setting}\s*=\s*)\d+\b",
            lambda match: f"{match[1]}{ANDROID_SDK_API}",
            patched_gradle,
        )
        if count != 1:
            print(f"Could not find a single {setting} value in {APP_GRADLE_PATH}", file=sys.stderr)
            return 1
    if patched_gradle != gradle_text:
        APP_GRADLE_PATH.write_text(patched_gradle, encoding="utf-8")
        print(f"Pinned Android compile and target SDK to {ANDROID_SDK_API}")

    FILE_PATHS_XML.parent.mkdir(parents=True, exist_ok=True)
    FILE_PATHS_XML.write_text(FILE_PATHS_CONTENT, encoding="utf-8")
    print(f"Wrote {FILE_PATHS_XML}")

    manifest_text = MANIFEST_PATH.read_text(encoding="utf-8")
    changed_manifest = False
    if "android.permission.WRITE_EXTERNAL_STORAGE" not in manifest_text and "<application" in manifest_text:
        manifest_text = manifest_text.replace(
            "<application", f"{LEGACY_WRITE_PERM}\n    <application", 1
        )
        changed_manifest = True
    if ".dadaduck.fileprovider" not in manifest_text:
        if "</application>" not in manifest_text:
            print(f"Could not find </application> in {MANIFEST_PATH}", file=sys.stderr)
            return 1
        manifest_text = manifest_text.replace(
            "</application>", f"{PROVIDER_BLOCK}    </application>", 1
        )
        changed_manifest = True
    if 'android:roundIcon="' not in manifest_text and 'android:icon="@mipmap/ic_launcher"' in manifest_text:
        manifest_text = manifest_text.replace(
            'android:icon="@mipmap/ic_launcher"',
            'android:icon="@mipmap/ic_launcher"\n        android:roundIcon="@mipmap/ic_launcher_round"',
            1,
        )
        changed_manifest = True
    application_tag = re.search(r"<application\b[^>]*>", manifest_text)
    if not application_tag:
        print(f"Could not find <application> in {MANIFEST_PATH}", file=sys.stderr)
        return 1
    attributes = {
        "allowBackup": "false",
        "fullBackupContent": "@xml/dadaduck_backup_rules",
        "dataExtractionRules": "@xml/dadaduck_data_extraction_rules",
    }
    patched_tag = application_tag.group()
    for name, value in attributes.items():
        attr = f'android:{name}="{value}"'
        pattern = rf'android:{name}="[^"]*"'
        if re.search(pattern, patched_tag):
            patched_tag = re.sub(pattern, attr, patched_tag)
        else:
            patched_tag = patched_tag.replace("<application", f"<application\n        {attr}", 1)
    if patched_tag != application_tag.group():
        manifest_text = manifest_text[:application_tag.start()] + patched_tag + manifest_text[application_tag.end():]
        changed_manifest = True

    excluded = ("root", "file", "database", "sharedpref", "external",
                "device_root", "device_file", "device_database", "device_sharedpref")
    excludes = "\n".join(f'        <exclude domain="{domain}" path="." />' for domain in excluded)
    legacy_excludes = "\n".join(f'    <exclude domain="{domain}" path="." />' for domain in excluded[:5])
    (FILE_PATHS_XML.parent / "dadaduck_backup_rules.xml").write_text(
        f"<full-backup-content>\n{legacy_excludes}\n</full-backup-content>\n", encoding="utf-8"
    )
    (FILE_PATHS_XML.parent / "dadaduck_data_extraction_rules.xml").write_text(
        "<data-extraction-rules>\n"
        f"    <cloud-backup>\n{excludes}\n    </cloud-backup>\n"
        f"    <device-transfer>\n{excludes}\n    </device-transfer>\n"
        "</data-extraction-rules>\n", encoding="utf-8"
    )
    if changed_manifest:
        MANIFEST_PATH.write_text(manifest_text, encoding="utf-8")
        print(f"Patched {MANIFEST_PATH}")

    # Remove Tauri CLI's default green robot icon XMLs and sync our generated Android icons cleanly.
    for stale_rel in (
        "drawable/ic_launcher_background.xml",
        "drawable-v24/ic_launcher_foreground.xml",
    ):
        stale_file = RES_DIR / stale_rel
        if stale_file.exists():
            stale_file.unlink()
            print(f"Removed template icon {stale_file}")

    if ICONS_ANDROID_DIR.exists():
        for src in ICONS_ANDROID_DIR.rglob("*"):
            if src.is_file():
                rel = src.relative_to(ICONS_ANDROID_DIR)
                dest = RES_DIR / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(src, dest)
        print(f"Synced Android launcher icons from {ICONS_ANDROID_DIR} to {RES_DIR}")

    matches = list(ANDROID_DIR.rglob("MainActivity.kt"))
    if not matches:
        print(f"MainActivity.kt not found under {ANDROID_DIR}", file=sys.stderr)
        return 1

    for main_activity_path in matches:
        existing = main_activity_path.read_text(encoding="utf-8")
        pkg_match = re.search(r"^package\s+([\w.]+)", existing, flags=re.MULTILINE)
        package_name = pkg_match.group(1) if pkg_match else "com.muxiaoxiii.dadaduck"
        main_activity_path.write_text(render_main_activity(package_name), encoding="utf-8")
        plugin_path = main_activity_path.parent / "AndroidStoragePlugin.kt"
        plugin_path.write_text(render_storage_plugin(package_name), encoding="utf-8")
        print(f"Patched {main_activity_path} and {plugin_path} (package={package_name})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
