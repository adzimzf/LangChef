package app.langchef

import android.os.Bundle
import androidx.activity.enableEdgeToEdge

import android.os.Build
import android.view.View
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.OnApplyWindowInsetsListener
import kotlin.math.max


object WindowInsetsUtil {
  fun applySystemBarsPadding(view: View){
    ViewCompat.setOnApplyWindowInsetsListener(
        view,
        OnApplyWindowInsetsListener { v: View?, windowInsets: WindowInsetsCompat? ->
            val systemBars = windowInsets!!.getInsets(WindowInsetsCompat.Type.systemBars())
            val ime = windowInsets.getInsets(WindowInsetsCompat.Type.ime())
            v!!.setPadding(
                systemBars.left,
                systemBars.top,
                systemBars.right,
                max(systemBars.bottom, ime.bottom)
            )
            WindowInsetsCompat.CONSUMED
        })
    }
  }


class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    window.decorView?.let { WindowInsetsUtil.applySystemBarsPadding(it) }
  }
}
