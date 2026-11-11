import React from "react";
import "./css/Box.css";

type Props = {
  children: React.ReactNode;
};

function Box({ children }: Props) {

    return (
        <div className="box">
            { children}
        </div>
    );
}

export default Box;