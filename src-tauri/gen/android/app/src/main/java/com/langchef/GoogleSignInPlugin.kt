package com.langchef

import android.app.Activity
import android.content.Intent
import android.util.Log
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.credentials.ClearCredentialStateRequest
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.NoCredentialException
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Invoke
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Plugin
import com.google.android.gms.auth.api.identity.AuthorizationClient
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.auth.api.identity.Identity
import com.google.android.gms.auth.api.identity.SignInClient
import com.google.android.gms.common.api.Scope
import com.google.android.libraries.identity.googleid.GetGoogleIdOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential
import com.google.gson.Gson
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.tasks.await
import kotlinx.coroutines.withContext
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit
import org.json.JSONObject

@InvokeArg
class SignInArgs {
    var clientId: String = ""
    var clientSecret: String? = null
    var scopes: List<String>? = null
    var flowType: String? = "native"
}

@InvokeArg
class SignOutArgs {
    var accessToken: String? = null
    var flowType: String? = "native"
}

@InvokeArg
class RefreshTokenArgs {
    var refreshToken: String? = null
    var clientId: String = ""
    var clientSecret: String? = null
    var scopes: List<String>? = null
    var flowType: String? = "native"
}

@TauriPlugin
class GoogleSignInPlugin(private val activity: Activity) : Plugin(activity) {

    companion object {
        private const val TAG = "GoogleSignInPlugin"

        const val CLIENT_ID = "clientId"
        const val CLIENT_SECRET = "clientSecret"
        const val SCOPES = "scopes"
        const val REDIRECT_URI = "redirectUri"
        const val AUTH_CODE = "authCode"
        const val GRANTED_SCOPES = "grantedScopes"
        const val ERROR_MESSAGE = "errorMessage"

        var RESULT_EXTRA_PREFIX = ""
    }

    private val scope = CoroutineScope(Dispatchers.Main)
    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()
    private val gson = Gson()
    private lateinit var authorizationClient: AuthorizationClient
    private lateinit var signInClient: SignInClient
    private lateinit var credentialManager: CredentialManager

    override fun load(webView: WebView) {
        super.load(webView)
        RESULT_EXTRA_PREFIX = activity.packageName + "."

        authorizationClient = Identity.getAuthorizationClient(activity)
        signInClient = Identity.getSignInClient(activity)
        credentialManager = CredentialManager.create(activity)
    }

    @Command
    fun signIn(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SignInArgs::class.java)

            if (args.clientId.isEmpty()) {
                invoke.reject("Client ID is required")
                return
            }

            if (args.flowType == "web") {
                if (args.clientSecret == null) {
                    invoke.reject("clientSecret is required for web flow")
                    return
                }
                signInWeb(invoke, args)
            } else {
                signInNative(invoke, args)
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start sign-in", e)
            invoke.reject("Failed to start sign-in: ${e.message}")
        }
    }

    private fun signInNative(invoke: Invoke, args: SignInArgs) {
        scope.launch {
            try {
                // Step 1: Get ID token + server auth code via CredentialManager (system account picker)
                val googleIdOption = GetGoogleIdOption.Builder()
                    .setServerClientId(args.clientId)
                    .setFilterByAuthorizedAccounts(false)
                    .build()

                val request = GetCredentialRequest.Builder()
                    .addCredentialOption(googleIdOption)
                    .build()

                val result = credentialManager.getCredential(activity, request)
                val credential = result.credential
                val googleIdTokenCredential = GoogleIdTokenCredential.createFrom(credential.data)
                val idToken = googleIdTokenCredential.idToken
                Log.d(TAG, "signInNative: got idToken_len=${idToken.length}, clientSecret=${if (args.clientSecret != null) "present" else "null"}")

                // Step 1b: Extract server auth code and exchange for tokens (access_token, refresh_token, id_token)
                val serverAuthCode = extractServerAuthCode(credential.data)
                val exchangedTokens = if (serverAuthCode != null && args.clientSecret != null) {
                    try {
                        exchangeAuthCodeForTokens(serverAuthCode, args.clientId, args.clientSecret, "")
                    } catch (e: Exception) {
                        Log.w(TAG, "Server auth code exchange failed, continuing with native flow: ${e.message}")
                        null
                    }
                } else {
                    null
                }
                Log.d(TAG, "args:${ args}")

                // Step 2: Get access token via AuthorizationClient for Drive scope
                startNativeAuthorization(invoke, idToken, args.scopes ?: emptyList(), exchangedTokens)

            } catch (e: GetCredentialCancellationException) {
                invoke.reject("Sign-in cancelled: ${e.message}")
            } catch (e: NoCredentialException) {
                invoke.reject("No Google account found: ${e.message}")
            } catch (e: GetCredentialException) {
                invoke.reject("Credential error [${e.type}]: ${e.message}")
            } catch (e: Exception) {
                invoke.reject("Sign-in failed [${e.javaClass.simpleName}]: ${e.message}")
            }
        }
    }

    /**
     * Extract the server auth code from credential extra data.
     * The Google ID library bundles it under the key "serverAuthCode".
     */
    private fun extractServerAuthCode(data: android.os.Bundle?): String? {
        if (data == null) return null
        return data.getString("serverAuthCode")
    }

    private fun signInWeb(invoke: Invoke, args: SignInArgs) {
        // On Android, web flow falls back to native (CredentialManager + AuthorizationClient)
        Log.w(TAG, "Web flow requested on Android, falling back to native sign-in")
        signInNative(invoke, args)
    }

    private var pendingNativeInvoke: Invoke? = null
    private var pendingIdToken: String? = null
    private var pendingExchangedTokens: Map<String, Any?>? = null

    private fun startNativeAuthorization(invoke: Invoke, idToken: String, scopes: List<String>, exchangedTokens: Map<String, Any?>? = null) {
        val authRequest = AuthorizationRequest.builder()
            .setRequestedScopes(scopes.map { Scope(it) })
            .build()

        authorizationClient.authorize(authRequest)
            .addOnSuccessListener { authResult ->
                if (authResult.hasResolution()) {
                    val pendingIntent = authResult.pendingIntent
                    if (pendingIntent != null) {
                        // Need to launch activity for user consent
                        pendingNativeInvoke = invoke
                        pendingIdToken = idToken
                        pendingExchangedTokens = exchangedTokens
                        val intent = Intent(activity, NativeSignInActivity::class.java).apply {
                            putExtra(NativeSignInActivity.EXTRA_PENDING_INTENT, pendingIntent)
                        }
                        startActivityForResult(invoke, intent, "nativeAuthorizationResult")
                    } else {
                        invoke.reject("No pending intent available for authorization")
                    }
                } else {
                    // Already authorized
                    val accessToken = authResult.accessToken
                    if (accessToken != null) {
                        val grantedScopes = authResult.grantedScopes.map { it.toString() }.toTypedArray()
                        resolveNativeSignIn(invoke, idToken, accessToken, grantedScopes, exchangedTokens)
                    } else {
                        invoke.reject("Failed to get access token")
                    }
                }
            }
            .addOnFailureListener { e ->
                Log.e(TAG, "Authorization failed", e)
                invoke.reject("Drive authorization failed: ${e.message}. Please try signing in again.")
            }
    }

    private fun resolveNativeSignIn(invoke: Invoke, idToken: String?, accessToken: String, grantedScopes: Array<String>, exchangedTokens: Map<String, Any?>? = null) {
        // Use exchanged tokens (from server auth code) for refresh_token and id_token if available,
        // otherwise fall back to the native values.
        val finalIdToken = exchangedTokens?.get("id_token") as? String ?: idToken ?: ""
        val refreshToken = exchangedTokens?.get("refresh_token") as? String ?: ""
        val expiresIn = (exchangedTokens?.get("expires_in") as? Number)?.toLong() ?: 3600L
        val expiresAt = System.currentTimeMillis() + (expiresIn * 1000)
        // Prefer the AuthorizationClient access token (has Drive scope) over the exchanged one
        val finalAccessToken = if (accessToken.isNotEmpty()) accessToken else (exchangedTokens?.get("access_token") as? String ?: "")

        val tokenObject = JSObject().apply {
            put("idToken", finalIdToken as Any)
            put("accessToken", finalAccessToken as Any)
            put("refreshToken", refreshToken as Any)
            put("expiresAt", expiresAt as Any)
            put("scopes", JSArray().apply {
                grantedScopes.forEach { scope -> put(scope as Any) }
            })
        }
        invoke.resolve(tokenObject)
    }

    @ActivityCallback
    private fun nativeAuthorizationResult(invoke: Invoke, result: ActivityResult) {
        val idToken = pendingIdToken
        val exchangedTokens = pendingExchangedTokens
        pendingNativeInvoke = null
        pendingIdToken = null
        pendingExchangedTokens = null

        if (result.resultCode == Activity.RESULT_CANCELED) {
            val error = result.data?.getStringExtra(NativeSignInActivity.RESULT_ERROR)
            invoke.reject(error ?: "Authorization cancelled")
            return
        }

        val data = result.data
        if (data == null) {
            invoke.reject("No data received from authorization")
            return
        }

        val accessToken = data.getStringExtra(NativeSignInActivity.RESULT_ACCESS_TOKEN)
        val grantedScopes = data.getStringArrayExtra(NativeSignInActivity.RESULT_GRANTED_SCOPES)

        if (accessToken == null) {
            invoke.reject("No access token received")
            return
        }

        resolveNativeSignIn(invoke, idToken, accessToken, grantedScopes ?: emptyArray(), exchangedTokens)
    }

    @Command
    fun signOut(invoke: Invoke) {
        scope.launch {
            try {
                val args = invoke.parseArgs(SignOutArgs::class.java)

                if (args.flowType == "web") {
                    signOutWeb(invoke, args)
                } else {
                    signOutNative(invoke, args)
                }
            } catch (e: Exception) {
                Log.e(TAG, "Sign-out failed", e)
                invoke.reject("Sign-out failed: ${e.message}")
            }
        }
    }

    private suspend fun signOutWeb(invoke: Invoke, args: SignOutArgs) {
        val accessToken = args.accessToken
        if (accessToken != null) {
            try {
                revokeAccessToken(accessToken)
                Log.d(TAG, "Access token revoked successfully")
            } catch (e: Exception) {
                Log.w(TAG, "Failed to revoke access token: ${e.message}")
            }
        }

        try {
            signInClient.signOut().await()
            Log.d(TAG, "Signed out from Google Sign-In client")
        } catch (e: Exception) {
            Log.w(TAG, "Failed to sign out from Google Sign-In client: ${e.message}")
        }

        val ret = JSObject()
        ret.put("success", true as Any)
        invoke.resolve(ret)
    }

    private suspend fun signOutNative(invoke: Invoke, args: SignOutArgs) {
        val accessToken = args.accessToken
        if (accessToken != null) {
            try {
                revokeAccessToken(accessToken)
                Log.d(TAG, "Access token revoked successfully")
            } catch (e: Exception) {
                Log.w(TAG, "Failed to revoke access token: ${e.message}")
            }
        }

        try {
            credentialManager.clearCredentialState(ClearCredentialStateRequest())
            Log.d(TAG, "Credential state cleared")
        } catch (e: Exception) {
            Log.w(TAG, "Failed to clear credential state: ${e.message}")
        }

        val ret = JSObject()
        ret.put("success", true as Any)
        invoke.resolve(ret)
    }

    @Command
    fun refreshToken(invoke: Invoke) {
        scope.launch {
            try {
                val args = invoke.parseArgs(RefreshTokenArgs::class.java)

                if (args.flowType == "web") {
                    if (args.clientSecret == null) {
                        invoke.reject("clientSecret is required for web flow")
                        return@launch
                    }
                    if (args.refreshToken == null) {
                        invoke.reject("refreshToken is required for web flow")
                        return@launch
                    }
                    refreshWeb(invoke, args)
                } else {
                    if (args.scopes.isNullOrEmpty()) {
                        invoke.reject("scopes is required for native flow refresh")
                        return@launch
                    }
                    refreshNative(invoke, args)
                }
            } catch (e: Exception) {
                Log.e(TAG, "Failed to refresh token", e)
                invoke.reject("Failed to refresh token: ${e.message}")
            }
        }
    }

    private suspend fun refreshWeb(invoke: Invoke, args: RefreshTokenArgs) {
        val tokenResponse = refreshAccessToken(args.refreshToken!!, args.clientId, args.clientSecret)
        val tokenObject = createTokenResponse(tokenResponse)
        invoke.resolve(tokenObject)
    }

    private suspend fun refreshNative(invoke: Invoke, args: RefreshTokenArgs) {
        Log.d(TAG, "refreshNative: refreshToken_len=${args.refreshToken?.length ?: 0}, scopes=${args.scopes}")

        // If we have a refresh token, use it (same as desktop refresh flow)
        if (!args.refreshToken.isNullOrEmpty()) {
            Log.d(TAG, "refreshNative: using refresh_token flow")
            val tokenResponse = refreshAccessToken(args.refreshToken!!, args.clientId, args.clientSecret)
            Log.d(TAG, "refreshNative: refreshAccessToken returned: accessToken_len=${(tokenResponse["access_token"] as? String)?.length ?: 0}, hasRefresh=${tokenResponse["refresh_token"] != null}, expiresIn=${tokenResponse["expires_in"]}")
            val tokenObject = createTokenResponse(tokenResponse)
            // The refreshed access_token only has OpenID scopes (openid, email).
            // We still need a Drive-scoped access token from AuthorizationClient.
            try {
                val authRequest = AuthorizationRequest.builder()
                    .setRequestedScopes(args.scopes!!.map { Scope(it) })
                    .build()
                Log.d(TAG, "refreshNative: calling AuthorizationClient.authorize for Drive scope...")
                val authResult = authorizationClient.authorize(authRequest).await()
                val driveAccessToken = authResult.accessToken
                Log.d(TAG, "refreshNative: AuthorizationClient succeeded, driveAccessToken_len=${driveAccessToken?.length ?: 0}")
                if (!driveAccessToken.isNullOrEmpty()) {
                    tokenObject.put("accessToken", driveAccessToken as Any)
                } else {
                    invoke.reject("Failed to obtain Drive access token during refresh. Please sign in again.")
                    return
                }
            } catch (e: Exception) {
                invoke.reject("Drive authorization failed during refresh: ${e.message}. Please sign in again.")
                return
            }
            Log.d(TAG, "refreshNative: resolving with accessToken_len=${(tokenObject["accessToken"] as? String)?.length ?: 0}, refreshToken_len=${(tokenObject["refreshToken"] as? String)?.length ?: 0}, expiresAt=${tokenObject["expiresAt"]}")
            invoke.resolve(tokenObject)
            return
        }

        Log.d(TAG, "refreshNative: no refresh_token, falling back to native re-auth flow")

        // Fallback: re-authorize via native APIs (no refresh token available)
        // Step 1: Get fresh ID token via CredentialManager (contains email in JWT payload)
        val idToken = try {
            val googleIdOption = GetGoogleIdOption.Builder()
                .setServerClientId(args.clientId)
                .setFilterByAuthorizedAccounts(true)
                .build()
            val request = GetCredentialRequest.Builder()
                .addCredentialOption(googleIdOption)
                .build()
            val result = credentialManager.getCredential(activity, request)
            val credential = result.credential
            val googleIdTokenCredential = GoogleIdTokenCredential.createFrom(credential.data)

            // Try to exchange server auth code for a refresh token
            val serverAuthCode = extractServerAuthCode(credential.data)
            if (serverAuthCode != null && args.clientSecret != null) {
                try {
                    val exchanged = exchangeAuthCodeForTokens(serverAuthCode, args.clientId, args.clientSecret, "")
                    val tokenObject = createTokenResponse(exchanged)
                    // Still need Drive scope access token from AuthorizationClient
                    val authRequest = AuthorizationRequest.builder()
                        .setRequestedScopes(args.scopes!!.map { Scope(it) })
                        .build()
                    val authResult = authorizationClient.authorize(authRequest).await()
                    val driveAccessToken = authResult.accessToken ?: ""
                    tokenObject.put("accessToken", driveAccessToken as Any)
                    invoke.resolve(tokenObject)
                    return
                } catch (e: Exception) {
                    Log.w(TAG, "Server auth code exchange during refresh failed: ${e.message}")
                }
            }

            googleIdTokenCredential.idToken
        } catch (e: Exception) {
            Log.w(TAG, "Failed to get ID token during refresh, continuing without it: ${e.message}")
            ""
        }

        // Step 2: Get access token via AuthorizationClient for Drive scope
        val authRequest = AuthorizationRequest.builder()
            .setRequestedScopes(args.scopes!!.map { Scope(it) })
            .build()

        val authResult = authorizationClient.authorize(authRequest).await()
        val accessToken = authResult.accessToken

        if (accessToken == null) {
            invoke.reject("Failed to get access token")
            return
        }

        val tokenObject = JSObject().apply {
            put("idToken", idToken as Any)
            put("accessToken", accessToken as Any)
            put("refreshToken", "" as Any)
            put("expiresAt", (System.currentTimeMillis() + 3600_000L )as Any)
            put("scopes", JSArray().apply {
                authResult.grantedScopes.forEach { scope -> put(scope.toString() as Any) }
            })
        }
        invoke.resolve(tokenObject)
    }

    private suspend fun exchangeAuthCodeForTokens(
        authCode: String,
        clientId: String,
        clientSecret: String?,
        redirectUri: String
    ): Map<String, Any?> = withContext(Dispatchers.IO) {
        val formBody = FormBody.Builder()
            .add("code", authCode)
            .add("client_id", clientId)
            .add("grant_type", "authorization_code")
            .add("redirect_uri", redirectUri)
            .apply {
                clientSecret?.let { add("client_secret", it) }
            }
            .build()

        val request = Request.Builder()
            .url("https://oauth2.googleapis.com/token")
            .post(formBody)
            .build()

        val response = httpClient.newCall(request).execute()

        if (!response.isSuccessful) {
            val errorBody = response.body?.string()
            throw Exception("Token exchange failed: $errorBody")
        }

        val responseBody = response.body?.string()
            ?: throw Exception("Empty response from token endpoint")

        gson.fromJson(responseBody, Map::class.java) as Map<String, Any?>
    }

    private suspend fun refreshAccessToken(
        refreshToken: String,
        clientId: String,
        clientSecret: String?
    ): Map<String, Any?> = withContext(Dispatchers.IO) {
        val formBody = FormBody.Builder()
            .add("refresh_token", refreshToken)
            .add("client_id", clientId)
            .add("grant_type", "refresh_token")
            .apply {
                clientSecret?.let { add("client_secret", it) }
            }
            .build()

        val request = Request.Builder()
            .url("https://oauth2.googleapis.com/token")
            .post(formBody)
            .build()

        val response = httpClient.newCall(request).execute()

        if (!response.isSuccessful) {
            val errorBody = response.body?.string()
            throw Exception("Token refresh failed: $errorBody")
        }

        val responseBody = response.body?.string()
            ?: throw Exception("Empty response from token endpoint")

        gson.fromJson(responseBody, Map::class.java) as Map<String, Any?>
    }

    private suspend fun revokeAccessToken(accessToken: String) = withContext(Dispatchers.IO) {
        val formBody = FormBody.Builder()
            .add("token", accessToken)
            .build()

        val request = Request.Builder()
            .url("https://oauth2.googleapis.com/revoke")
            .post(formBody)
            .build()

        val response = httpClient.newCall(request).execute()

        if (!response.isSuccessful) {
            val errorBody = response.body?.string()
            Log.w(TAG, "Token revocation response: $errorBody")
            // Google's revocation endpoint returns 400 if token is already invalid
            if (response.code != 400) {
                throw Exception("Token revocation failed with code ${response.code}")
            }
        }
    }

    private fun createTokenResponse(tokenResponse: Map<String, Any?>, grantedScopes: List<String>? = null): JSObject {
        val expiresIn = (tokenResponse["expires_in"] as? Number)?.toLong() ?: 3600
        val expiresAt = System.currentTimeMillis() + (expiresIn * 1000)

        return JSObject().apply {
            put("idToken", (tokenResponse["id_token"] as? String ?: "") as Any)
            put("accessToken", (tokenResponse["access_token"] as? String ?: "") as Any)
            put("refreshToken", (tokenResponse["refresh_token"] as? String ?: "") as Any)
            put("expiresAt", expiresAt as Any)

            val scopes = grantedScopes ?: (tokenResponse["scope"] as? String)?.split(" ") ?: emptyList()
            put("scopes", JSArray().apply {
                scopes.forEach { scope -> put(scope as Any) }
            })
        }
    }
}
