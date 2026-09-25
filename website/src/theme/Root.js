import React from 'react';

// Official extension point: wraps every page, so fonts load site-wide.
import '@fontsource-variable/inter';
import '@fontsource-variable/jetbrains-mono';

export default function Root({ children }) {
  return <>{children}</>;
}
