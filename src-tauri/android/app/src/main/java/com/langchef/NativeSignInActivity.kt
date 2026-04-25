package com.langchef

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.util.Log
import com.google.android.gms.auth.api.identity.AuthorizationClient
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.Identity

class NativeSignInActivity : Activity() {
    private lateinit var authorizationClient: AuthorizationClient

    companion object {
        const val TAG = "NativeSignInActivity"
        private var pendingInvoke: tauri.Invoke? = null
        private var pendingIdToken: String? = null

        fun setPendingData(invoke: tauri.Invoke, idToken: String) {
            pendingInvoke = invoke
            pendingIdToken = idToken
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        authorizationClient = Identity.getAuthorizationClient(applicationContext)

        val authRequest = intent.getParcelableExtra<AuthorizationRequest>("authorizationRequest")
            ?: run {
                Log.e(TAG, "No authorization request provided")
                finishWithFailure("Missing authorization request")
                return
            }

        authorizationClient.authorize(authRequest)
            .addOnSuccessListener { result ->
                if (result.hasResolution()) {
                    // Still needs resolution (shouldn't happen in this activity but handle it)
                    result.resolution?.startIntentSenderForResult(this, 1001, null, 0, 0, 0)
                } else {
                    val accessToken = result.accessToken
                    val grantedScopes = result.grantedScopes?.map { it.scope } ?: emptyList()
                    finishWithSuccess(accessToken, grantedScopes)
                }
            }
            .addOnFailureListener { e ->
                Log.e(TAG, "Authorization failed", e)
                finishWithFailure(e.message ?: "Authorization failed")
            }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)

        if (requestCode == 1001) {
            // Handle resolution result
            val authResult = authorizationClient.getAuthorizationResultFromIntent(data)
            if (authResult != null) {
                val accessToken = authResult.accessToken
                val grantedScopes = authResult.grantedScopes?.map { it.scope } ?: emptyList()
                finishWithSuccess(accessToken, grantedScopes)
            } else {
                finishWithFailure("Authorization cancelled")
            }
        } else {
            finishWithFailure("Unexpected result")
        }
    }

    private fun finishWithSuccess(accessToken: String?, grantedScopes: List<String>) {
        val invoke = pendingInvoke
        val idToken = pendingIdToken

        if (invoke != null && idToken != null) {
            try {
                val response = org.json.JSONObject().apply {
                    put("idToken", idToken)
                    put("accessToken", accessToken ?: "")
                    put("refreshToken", "")
                    put("scopes", org.json.JSONArray(grantedScopes))
                    put("expiresAt", 0)
                }
                invoke.resolve(response)
            } catch (e: Exception) {
                Log.e(TAG, "Failed to resolve invoke", e)
            }
        }

        pendingInvoke = null
        pendingIdToken = null
        finish()
    }

    private fun finishWithFailure(error: String) {
        val invoke = pendingInvoke
        if (invoke != null) {
            try {
                invoke.reject(error)
            } catch (e: Exception) {
                Log.e(TAG, "Failed to reject invoke", e)
            }
        }

        pendingInvoke = null
        pendingIdToken = null
        finish()
    }
}
