package com.langchef

import android.app.Activity
import android.content.Intent
import android.util.Log
import com.google.android.gms.auth.api.identity.AuthorizationClient
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.auth.api.identity.Identity
import com.google.android.gms.common.Scopes
import com.google.android.gms.common.api.Scope
import com.google.android.gms.credentials.CredentialManager
import com.google.android.gms.credentials.GetCredentialRequest
import com.google.android.gms.credentials.GetGoogleIdOption
import com.google.android.gms.credentials.GoogleIdTokenCredential
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONObject
import tauri.JsObject
import tauri.Invoke

class GoogleSignInPlugin(private val activity: Activity) {
    private val scope = CoroutineScope(Dispatchers.Main)
    private val authorizationClient: AuthorizationClient by lazy {
        Identity.getAuthorizationClient(activity.applicationContext)
    }
    private val credentialManager: CredentialManager by lazy {
        CredentialManager.create(activity)
    }

    companion object {
        const val TAG = "GoogleSignInPlugin"
        const val CLIENT_ID = "clientId"
        const val CLIENT_SECRET = "clientSecret"
        const val SCOPES = "scopes"
        const val FLOW_TYPE = "flowType"
    }

    fun signIn(invoke: Invoke) {
        scope.launch {
            try {
                val args = invoke.parseArgs(SignInArgs::class.java)

                // Step 1: Get ID token via CredentialManager (system account picker)
                val googleIdOption = GetGoogleIdOption.Builder()
                    .setServerClientId(args.clientId!!)
                    .setFilterByAuthorizedAccounts(false)
                    .build()

                val request = GetCredentialRequest.Builder()
                    .addCredentialOption(googleIdOption)
                    .build()

                val result = credentialManager.getCredential(activity, request)
                val credential = result.credential
                val idToken = GoogleIdTokenCredential.createFrom(credential.data).idToken

                // Step 2: Get access token via AuthorizationClient for Drive scope
                startNativeAuthorization(invoke, idToken, args.scopes ?: emptyList())
            } catch (e: Exception) {
                Log.e(TAG, "Sign-in failed", e)
                invoke.reject(e.message ?: "Sign-in failed")
            }
        }
    }

    private fun startNativeAuthorization(
        invoke: Invoke,
        idToken: String,
        scopes: List<String>
    ) {
        val requestedScopes = scopes.map { Scope(it) }.toTypedArray()

        if (requestedScopes.isEmpty()) {
            // No additional scopes needed, return what we have
            resolveNativeSignIn(invoke, idToken, null, emptyList())
            return
        }

        val authRequest = AuthorizationRequest.Builder()
            .setRequestedScopes(*requestedScopes)
            .build()

        authorizationClient.authorize(authRequest)
            .addOnSuccessListener { authResult ->
                if (authResult.hasResolution()) {
                    // Need user consent - launch resolution activity
                    val intent = Intent(activity, NativeSignInActivity::class.java).apply {
                        putExtra("authorizationRequest", authRequest)
                    }
                    startActivityForResult(invoke, intent, idToken, "nativeAuthorizationResult")
                } else {
                    // Already authorized - get access token directly
                    val accessToken = authResult.accessToken
                    val grantedScopes = authResult.grantedScopes?.map { it.scope } ?: emptyList()
                    resolveNativeSignIn(invoke, idToken, accessToken, grantedScopes)
                }
            }
            .addOnFailureListener { e ->
                Log.e(TAG, "Authorization failed", e)
                // Fallback: return just id token without drive scope
                resolveNativeSignIn(invoke, idToken, null, emptyList())
            }
    }

    private fun resolveNativeSignIn(
        invoke: Invoke,
        idToken: String,
        accessToken: String?,
        grantedScopes: List<String>
    ) {
        // Fetch email from ID token (or use a separate API call)
        val response = JSONObject().apply {
            put("idToken", idToken)
            put("accessToken", accessToken ?: "")
            put("refreshToken", "") // Native flow doesn't provide refresh token
            put("scopes", JSONArray(grantedScopes))
            put("expiresAt", 0)
        }
        invoke.resolve(response)
    }

    fun signOut(invoke: Invoke) {
        scope.launch {
            try {
                credentialManager.clearCredentialState(ClearCredentialStateRequest())
                invoke.resolve(JSONObject().put("success", true))
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Sign-out failed")
            }
        }
    }

    private fun startActivityForResult(
        invoke: Invoke,
        intent: Intent,
        idToken: String,
        key: String
    ) {
        // Use Tauri's activity result handling if available,
        // otherwise fall back to a simple approach
        @Suppress("DEPRECATION")
        activity.startActivityForResult(intent, key.hashCode())
        // Note: In a full Tauri plugin integration, you'd use
        // Tauri's PluginActivity.registerForActivityResult or similar.
        // For now, this is a simplified version.
    }

    data class SignInArgs(
        val clientId: String? = null,
        val clientSecret: String? = null,
        val scopes: List<String>? = null,
        val flowType: String? = "native",
    )
}
