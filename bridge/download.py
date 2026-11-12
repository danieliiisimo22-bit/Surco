"""Use OS certificate trust for the unmodified yt-dlp command line."""

import truststore

truststore.inject_into_ssl()
import yt_dlp

yt_dlp.main()
