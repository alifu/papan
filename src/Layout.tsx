import React, { useState } from 'react';
import './css/Layout.css';

type Props = {
  children: React.ReactNode;
};

function Layout({ children }: Props) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="layout">
      <div className={`sidebar ${collapsed ? 'collapsed' : ''}`}>
        <button className="toggle-btn" onClick={() => setCollapsed(!collapsed)}>
          {collapsed ? '→' : '←'}
        </button>
        {!collapsed && <div className="sidebar-content">Project Navigation</div>}
      </div>

      <div className="main">{children}</div>
    </div>
  );
};

export default Layout;